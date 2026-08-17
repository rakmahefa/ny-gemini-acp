//! MCP tool infrastructure.
//!
//! The runtime targets MCP `2026-07-28`: requests are self-describing and
//! stateless at the protocol layer. MCP remains behind the existing
//! `ToolRegistry` surface so builtin and remote tools share one execution
//! contract.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};

const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
const CLIENT_NAME: &str = "gemini-acp";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGE_COUNT: usize = 10_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const IO_TIMEOUT: Duration = Duration::from_secs(120);
const CACHE_DEFAULT_TTL: Duration = Duration::ZERO;
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("MCP configuration is invalid: {0}")]
    Config(String),
    #[error("MCP transport '{transport}' failed: {message}")]
    Transport { transport: String, message: String },
    #[error("MCP protocol error: {0}")]
    Protocol(String),
    #[error("MCP server rejected request: code={code}, message={message}")]
    Remote { code: i64, message: String },
    #[error("MCP response exceeded {MAX_MESSAGE_BYTES} bytes")]
    MessageTooLarge,
    #[error("MCP pagination exceeded {MAX_PAGE_COUNT} pages")]
    PaginationLimit,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportKind {
    Stdio,
    Http,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportKind,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl McpServerConfig {
    fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() {
            return Err(McpError::Config("server name is empty".into()));
        }
        if self.name.chars().any(char::is_control) {
            return Err(McpError::Config(format!(
                "server '{}' contains control characters",
                self.name
            )));
        }
        match self.transport {
            McpTransportKind::Stdio
                if self.command.as_deref().unwrap_or("").trim().is_empty() =>
            {
                Err(McpError::Config(format!(
                    "stdio server '{}' is missing command",
                    self.name
                )))
            }
            McpTransportKind::Http if self.url.as_deref().unwrap_or("").trim().is_empty() => {
                Err(McpError::Config(format!(
                    "http server '{}' is missing url",
                    self.name
                )))
            }
            _ => {
                validate_custom_headers(&self.headers)?;
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(rename = "outputSchema", default)]
    pub output_schema: Option<Value>,
}

impl McpToolDescriptor {
    fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() {
            return Err(McpError::Protocol("MCP tool descriptor has empty name".into()));
        }
        if self.name.chars().any(char::is_control) {
            return Err(McpError::Protocol(format!(
                "MCP tool '{}' contains control characters",
                self.name
            )));
        }
        if !self.input_schema.is_object() {
            return Err(McpError::Protocol(format!(
                "MCP tool '{}' has a non-object inputSchema root",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct RpcResponse {
    id: Value,
    result: Option<Value>,
    error: Option<RpcErrorObject>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcErrorObject {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

#[derive(Debug, Clone)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

impl<'a> Serialize for RpcRequest<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(
            &json!({
                "jsonrpc": self.jsonrpc,
                "id": self.id,
                "method": self.method,
                "params": self.params,
            }),
            serializer,
        )
    }
}

#[derive(Debug)]
struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioTransport {
    async fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let command = config
            .command
            .as_deref()
            .ok_or_else(|| McpError::Config("missing stdio command".into()))?;
        let mut child_command = Command::new(command);
        child_command
            .args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        if let Some(cwd) = &config.cwd {
            child_command.current_dir(cwd);
        }
        for (key, value) in &config.env {
            child_command.env(key, value);
        }
        let mut child = child_command.spawn().map_err(|error| McpError::Transport {
            transport: "stdio".into(),
            message: error.to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| McpError::Transport {
            transport: "stdio".into(),
            message: "child stdin unavailable".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Transport {
            transport: "stdio".into(),
            message: "child stdout unavailable".into(),
        })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    async fn request(&mut self, request: &RpcRequest<'_>) -> Result<RpcResponse, McpError> {
        let line = serialize_request_line(request)?;
        let write = async {
            self.stdin
                .write_all(&line)
                .await
                .map_err(|error| McpError::Transport {
                    transport: "stdio".into(),
                    message: error.to_string(),
                })?;
            self.stdin
                .flush()
                .await
                .map_err(|error| McpError::Transport {
                    transport: "stdio".into(),
                    message: error.to_string(),
                })
        };
        tokio::time::timeout(IO_TIMEOUT, write)
            .await
            .map_err(|_| McpError::Transport {
                transport: "stdio".into(),
                message: "request write timeout".into(),
            })??;

        let mut response_line = String::new();
        let read = async {
            loop {
                response_line.clear();
                let read = self
                    .stdout
                    .read_line(&mut response_line)
                    .await
                    .map_err(|error| McpError::Transport {
                        transport: "stdio".into(),
                        message: error.to_string(),
                    })?;
                if read == 0 {
                    let status = self.child.try_wait().ok().flatten();
                    return Err(McpError::Transport {
                        transport: "stdio".into(),
                        message: format!("server closed stdout (status={status:?})"),
                    });
                }
                if response_line.len() > MAX_MESSAGE_BYTES {
                    return Err(McpError::MessageTooLarge);
                }
                if response_line.trim().is_empty() {
                    continue;
                }
                break Ok::<(), McpError>(());
            }
        };
        tokio::time::timeout(IO_TIMEOUT, read)
            .await
            .map_err(|_| McpError::Transport {
                transport: "stdio".into(),
                message: "response read timeout".into(),
            })??;
        parse_json_rpc_response(response_line.as_bytes(), None)
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[derive(Debug)]
struct HttpTransport {
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
}

impl HttpTransport {
    fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let url = config
            .url
            .clone()
            .ok_or_else(|| McpError::Config("missing http url".into()))?;
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| McpError::Transport {
                transport: "http".into(),
                message: format!("HTTP client initialization failed: {error}"),
            })?;
        Ok(Self {
            client,
            url,
            headers: config.headers.clone(),
        })
    }

    async fn request(
        &self,
        request: &RpcRequest<'_>,
        method: &str,
        tool_name: Option<&str>,
    ) -> Result<RpcResponse, McpError> {
        let payload = serialize_request_payload(request)?;
        let mut builder = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .header("Mcp-Method", method);
        if let Some(tool_name) = tool_name {
            builder = builder.header("Mcp-Name", tool_name);
        }
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .body(payload)
            .send()
            .await
            .map_err(|error| McpError::Transport {
                transport: "http".into(),
                message: error.to_string(),
            })?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let body = response
            .bytes()
            .await
            .map_err(|error| McpError::Transport {
                transport: "http".into(),
                message: error.to_string(),
            })?;
        if body.len() > MAX_MESSAGE_BYTES {
            return Err(McpError::MessageTooLarge);
        }
        if !status.is_success() && body.is_empty() {
            return Err(McpError::Transport {
                transport: "http".into(),
                message: format!("HTTP {status}"),
            });
        }
        let response = if content_type.contains("text/event-stream") {
            parse_sse_rpc_response(&body, Some(request.id))?
        } else {
            parse_json_rpc_response(&body, Some(request.id))?
        };
        if !status.is_success() {
            if let Some(error) = response.error.clone() {
                return Err(McpError::Remote {
                    code: error.code,
                    message: error.message,
                });
            }
            return Err(McpError::Transport {
                transport: "http".into(),
                message: format!("HTTP {status}"),
            });
        }
        Ok(response)
    }
}

#[derive(Debug)]
enum McpTransport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl McpTransport {
    async fn request(
        &mut self,
        request: &RpcRequest<'_>,
        method: &str,
        tool_name: Option<&str>,
    ) -> Result<RpcResponse, McpError> {
        match self {
            Self::Stdio(transport) => transport.request(request).await,
            Self::Http(transport) => transport.request(request, method, tool_name).await,
        }
    }
}

#[derive(Debug)]
struct CachedToolList {
    tools: Vec<McpToolDescriptor>,
    expires_at: Instant,
}

#[derive(Debug)]
struct McpServerClient {
    transport: McpTransport,
    next_id: u64,
    cached_tools: Option<CachedToolList>,
}

impl McpServerClient {
    async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        config.validate()?;
        let transport = match config.transport {
            McpTransportKind::Stdio => McpTransport::Stdio(StdioTransport::connect(&config).await?),
            McpTransportKind::Http => McpTransport::Http(HttpTransport::connect(&config)?),
        };
        Ok(Self {
            transport,
            next_id: 1,
            cached_tools: None,
        })
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn request_params(&self, params: Value) -> Value {
        let mut params = match params {
            Value::Object(object) => object,
            _ => serde_json::Map::new(),
        };
        params.insert(
            "_meta".into(),
            json!({
                META_PROTOCOL_VERSION: MCP_PROTOCOL_VERSION,
                META_CLIENT_INFO: {"name": CLIENT_NAME, "version": CLIENT_VERSION},
                META_CLIENT_CAPABILITIES: {}
            }),
        );
        Value::Object(params)
    }

    async fn request(&mut self, method: &str, tool_name: Option<&str>, params: Value) -> Result<Value, McpError> {
        let id = self.next_request_id();
        let request = RpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params: self.request_params(params),
        };
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            self.transport.request(&request, method, tool_name),
        )
        .await
        .map_err(|_| McpError::Transport {
            transport: "mcp".into(),
            message: format!("request '{method}' timed out"),
        })??;
        if let Some(error) = response.error {
            return Err(McpError::Remote {
                code: error.code,
                message: error.message,
            });
        }
        response
            .result
            .ok_or_else(|| McpError::Protocol(format!("MCP response for '{method}' has no result")))
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolDescriptor>, McpError> {
        if let Some(cache) = &self.cached_tools {
            if Instant::now() < cache.expires_at {
                return Ok(cache.tools.clone());
            }
        }
        let mut cursor: Option<String> = None;
        let mut tools = Vec::new();
        let mut seen_cursors = HashSet::new();
        for _ in 0..MAX_PAGE_COUNT {
            let mut params = json!({"limit": 100});
            if let Some(cursor) = &cursor {
                params["cursor"] = Value::String(cursor.clone());
            }
            let result = self.request("tools/list", None, params).await?;
            let page: ToolListPage = serde_json::from_value(result)
                .map_err(|error| McpError::Protocol(format!("invalid tools/list result: {error}")))?;
            for descriptor in page.tools {
                descriptor.validate()?;
                tools.push(descriptor);
            }
            match page.next_cursor {
                Some(next) if !next.is_empty() => {
                    if !seen_cursors.insert(next.clone()) {
                        return Err(McpError::Protocol("tools/list pagination cursor repeated".into()));
                    }
                    cursor = Some(next);
                }
                _ => {
                    let expires_at = Instant::now()
                        + page
                            .ttl_ms
                            .map(Duration::from_millis)
                            .unwrap_or(CACHE_DEFAULT_TTL);
                    self.cached_tools = Some(CachedToolList {
                        tools: tools.clone(),
                        expires_at,
                    });
                    return Ok(tools);
                }
            }
        }
        Err(McpError::PaginationLimit)
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<ToolCallResult, McpError> {
        if !arguments.is_object() {
            return Err(McpError::Protocol(format!(
                "MCP tools/call arguments for '{name}' must be an object"
            )));
        }
        let result = self
            .request("tools/call", Some(name), json!({"name": name, "arguments": arguments}))
            .await?;
        serde_json::from_value(result)
            .map_err(|error| McpError::Protocol(format!("invalid tools/call result: {error}")))
    }
}

#[derive(Debug, Deserialize)]
struct ToolListPage {
    #[serde(default)]
    tools: Vec<McpToolDescriptor>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<String>,
    #[serde(rename = "ttlMs", default)]
    ttl_ms: Option<u64>,
    #[serde(rename = "cacheScope", default)]
    _cache_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallResult {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default, rename = "structuredContent")]
    structured_content: Option<Value>,
    #[serde(default, rename = "isError")]
    is_error: bool,
    #[serde(default, rename = "resultType")]
    result_type: Option<String>,
    #[serde(default, rename = "requestState")]
    request_state: Option<String>,
    #[serde(default, rename = "inputRequests")]
    input_requests: Option<Value>,
}

impl ToolCallResult {
    fn into_tool_result(self) -> crate::tools::registry::ToolResult {
        if matches!(self.result_type.as_deref(), Some("input_required")) {
            let details = json!({
                "resultType": "input_required",
                "inputRequests": self.input_requests,
                "requestState": self.request_state,
            });
            return crate::tools::registry::ToolResult::Err(details.to_string());
        }
        let rendered = render_tool_content(&self.content, self.structured_content.as_ref());
        if self.is_error {
            crate::tools::registry::ToolResult::Err(rendered)
        } else {
            crate::tools::registry::ToolResult::Ok(rendered)
        }
    }
}

#[derive(Debug)]
struct McpRegisteredTool {
    server_index: usize,
    server_name: String,
    descriptor: McpToolDescriptor,
}

#[derive(Debug)]
pub struct McpCatalog {
    servers: Vec<Mutex<McpServerClient>>,
    tools: Vec<McpRegisteredTool>,
}

impl McpCatalog {
    pub async fn from_env() -> Result<Self, McpError> {
        let raw = match std::env::var("GEMINI_ACP_MCP_SERVERS") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return Ok(Self { servers: Vec::new(), tools: Vec::new() }),
        };
        let configs: Vec<McpServerConfig> = serde_json::from_str(&raw)
            .map_err(|error| McpError::Config(format!("invalid GEMINI_ACP_MCP_SERVERS: {error}")))?;
        Self::from_configs(configs).await
    }

    pub async fn from_configs(configs: Vec<McpServerConfig>) -> Result<Self, McpError> {
        let mut servers = Vec::with_capacity(configs.len());
        let mut tools = Vec::new();
        let mut server_names = HashSet::new();
        let mut qualified_names = HashSet::new();
        for (server_index, config) in configs.into_iter().enumerate() {
            let server_name = config.name.clone();
            config.validate()?;
            if !server_names.insert(server_name.clone()) {
                return Err(McpError::Config(format!(
                    "duplicate MCP server name '{}', server names must be unique",
                    server_name
                )));
            }
            let mut client = McpServerClient::connect(config).await?;
            let descriptors = client.list_tools().await?;
            for descriptor in descriptors {
                let qualified = qualified_name(&server_name, &descriptor.name);
                if !qualified_names.insert(qualified.clone()) {
                    return Err(McpError::Config(format!(
                        "duplicate MCP qualified tool name '{qualified}'"
                    )));
                }
                tools.push(McpRegisteredTool {
                    server_index,
                    server_name: server_name.clone(),
                    descriptor,
                });
            }
            servers.push(Mutex::new(client));
        }
        Ok(Self { servers, tools })
    }

    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    pub fn definitions(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|tool| {
                let mut definition = json!({
                    "name": qualified_name(&tool.server_name, &tool.descriptor.name),
                    "description": tool.descriptor.description,
                    "parameters": tool.descriptor.input_schema,
                });
                if let Some(output_schema) = &tool.descriptor.output_schema {
                    definition["outputSchema"] = output_schema.clone();
                }
                definition
            })
            .collect()
    }

    pub async fn call_async(
        &self,
        qualified: &str,
        args: &Value,
        _cwd: &Path,
        _extra_dirs: &[PathBuf],
    ) -> Option<crate::tools::registry::ToolResult> {
        let binding = self.tools.iter().find(|binding| {
            qualified_name(&binding.server_name, &binding.descriptor.name) == qualified
        })?;
        let server = self.servers.get(binding.server_index)?;
        let mut server = server.lock().await;
        match server.call_tool(&binding.descriptor.name, args.clone()).await {
            Ok(result) => Some(result.into_tool_result()),
            Err(error) => Some(crate::tools::registry::ToolResult::Err(error.to_string())),
        }
    }
}

fn validate_custom_headers(headers: &HashMap<String, String>) -> Result<(), McpError> {
    const RESERVED: &[&str] = &[
        "content-type",
        "accept",
        "mcp-protocol-version",
        "mcp-method",
        "mcp-name",
        "mcp-session-id",
    ];
    for (name, value) in headers {
        let parsed = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| McpError::Config(format!("invalid MCP HTTP header '{name}': {error}")))?;
        HeaderValue::from_str(value)
            .map_err(|error| McpError::Config(format!("invalid value for MCP HTTP header '{name}': {error}")))?;
        if RESERVED.iter().any(|reserved| parsed.as_str().eq_ignore_ascii_case(reserved)) {
            return Err(McpError::Config(format!(
                "MCP HTTP header '{}' is reserved and cannot be overridden",
                name
            )));
        }
    }
    Ok(())
}

fn serialize_request_payload(request: &RpcRequest<'_>) -> Result<Vec<u8>, McpError> {
    let payload = serde_json::to_vec(request)
        .map_err(|error| McpError::Protocol(format!("serialize request: {error}")))?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(McpError::MessageTooLarge);
    }
    Ok(payload)
}

fn serialize_request_line(request: &RpcRequest<'_>) -> Result<Vec<u8>, McpError> {
    let mut payload = serialize_request_payload(request)?;
    payload.push(b'\n');
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(McpError::MessageTooLarge);
    }
    Ok(payload)
}

fn parse_json_rpc_response(bytes: &[u8], expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    parse_json_rpc_value(value, expected_id)
}

fn parse_sse_rpc_response(bytes: &[u8], expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let mut data_lines = Vec::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_owned());
            continue;
        }
        if line.trim().is_empty() && !data_lines.is_empty() {
            let data = data_lines.join("\n");
            let value: Value = serde_json::from_str(&data)
                .map_err(|error| McpError::Protocol(format!("invalid SSE JSON-RPC data: {error}")))?;
            return parse_json_rpc_value(value, expected_id);
        }
    }
    if !data_lines.is_empty() {
        let data = data_lines.join("\n");
        let value: Value = serde_json::from_str(&data)
            .map_err(|error| McpError::Protocol(format!("invalid SSE JSON-RPC data: {error}")))?;
        return parse_json_rpc_value(value, expected_id);
    }
    Err(McpError::Protocol("SSE response contained no data event".into()))
}

fn parse_json_rpc_value(value: Value, expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let object = value
        .as_object()
        .ok_or_else(|| McpError::Protocol("JSON-RPC response is not an object".into()))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(McpError::Protocol("JSON-RPC response has invalid jsonrpc version".into()));
    }
    let id = object
        .get("id")
        .cloned()
        .ok_or_else(|| McpError::Protocol("JSON-RPC response has no id".into()))?;
    if let Some(expected_id) = expected_id {
        if id != Value::from(expected_id) {
            return Err(McpError::Protocol(format!(
                "JSON-RPC response id mismatch: expected {expected_id}, got {id}"
            )));
        }
    }
    let result = object.get("result").cloned();
    let error = object
        .get("error")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC error object: {error}")))?;
    match (result.is_some(), error.is_some()) {
        (true, true) => Err(McpError::Protocol(
            "JSON-RPC response contains both result and error".into(),
        )),
        (false, false) => Err(McpError::Protocol(
            "JSON-RPC response contains neither result nor error".into(),
        )),
        _ => Ok(RpcResponse { id, result, error }),
    }
}

fn render_tool_content(content: &[Value], structured_content: Option<&Value>) -> String {
    let mut rendered = Vec::new();
    for value in content {
        match value.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    rendered.push(text.to_owned());
                }
            }
            Some("resource") => {
                if let Some(text) = value
                    .get("resource")
                    .and_then(|resource| resource.get("text"))
                    .and_then(Value::as_str)
                {
                    rendered.push(text.to_owned());
                } else {
                    rendered.push(serde_json::to_string(value).unwrap_or_else(|_| "<invalid MCP resource>".into()));
                }
            }
            _ => rendered.push(
                serde_json::to_string(value)
                    .unwrap_or_else(|_| "<unserializable MCP content>".into()),
            ),
        }
    }
    if rendered.is_empty() {
        if let Some(structured_content) = structured_content {
            rendered.push(
                serde_json::to_string(structured_content)
                    .unwrap_or_else(|_| "<unserializable MCP structuredContent>".into()),
            );
        }
    } else if let Some(structured_content) = structured_content {
        rendered.push(serde_json::to_string(structured_content).unwrap_or_else(|_| {
            "<unserializable MCP structuredContent>".into()
        }));
    }
    rendered.join("\n")
}

fn qualified_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", sanitize_component(server), sanitize_component(tool))
}

fn sanitize_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            output.push(ch);
        } else {
            let mut bytes = [0_u8; 4];
            for byte in ch.encode_utf8(&mut bytes).as_bytes() {
                use std::fmt::Write as _;
                let _ = write!(output, "_{byte:02x}");
            }
        }
    }
    if output.is_empty() { "_".into() } else { output }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_names_are_collision_safe() {
        assert_eq!(qualified_name("foo-bar", "read_file"), "mcp__foo-bar__read_file");
        assert_eq!(qualified_name("foo/bar", "read file"), "mcp__foo_2fbar__read_20file");
        assert_ne!(qualified_name("foo/bar", "read_file"), qualified_name("foo_bar", "read_file"));
    }

    #[test]
    fn validates_reserved_http_headers() {
        let mut headers = HashMap::new();
        headers.insert("Mcp-Session-Id".into(), "forbidden".into());
        assert!(validate_custom_headers(&headers).is_err());
    }

    #[test]
    fn request_payload_is_self_describing() {
        let client = McpServerClient {
            transport: McpTransport::Http(HttpTransport {
                client: reqwest::Client::new(),
                url: "http://localhost".into(),
                headers: HashMap::new(),
            }),
            next_id: 1,
            cached_tools: None,
        };
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/list",
            params: client.request_params(json!({"limit": 100})),
        };
        let value: Value = serde_json::from_slice(&serialize_request_payload(&request).unwrap()).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["params"]["_meta"][META_PROTOCOL_VERSION], MCP_PROTOCOL_VERSION);
        assert_eq!(value["params"]["_meta"][META_CLIENT_INFO]["name"], CLIENT_NAME);
    }

    #[test]
    fn rejects_invalid_json_rpc_response_shape() {
        let err = parse_json_rpc_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"bad"}}"#,
            Some(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("both result and error"));
    }

    #[test]
    fn rejects_response_id_mismatch() {
        let err = parse_json_rpc_response(br#"{"jsonrpc":"2.0","id":2,"result":{}}"#, Some(1)).unwrap_err();
        assert!(err.to_string().contains("id mismatch"));
    }

    #[test]
    fn renders_content_blocks_without_leaking_raw_text_blocks() {
        let content = vec![
            json!({"type":"text","text":"hello"}),
            json!({"type":"resource","resource":{"uri":"file:///x","text":"world"}}),
        ];
        assert_eq!(render_tool_content(&content, None), "hello\nworld");
    }

    #[test]
    fn renders_structured_content_when_no_text_exists() {
        let structured = json!({"answer": 42});
        assert_eq!(render_tool_content(&[], Some(&structured)), r#"{"answer":42}"#);
    }

    #[test]
    fn tool_descriptor_requires_object_input_schema() {
        let descriptor = McpToolDescriptor {
            name: "x".into(),
            description: String::new(),
            input_schema: json!([]),
            output_schema: None,
        };
        assert!(descriptor.validate().is_err());
    }
}
