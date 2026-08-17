//! MCP tool infrastructure.
//!
//! This module deliberately keeps MCP behind the existing runtime tool surface.
//! Builtin tools remain native `Tool` implementations while MCP tools are
//! discovered dynamically and exposed through the same `ToolRegistry` API.

use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};

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
}

#[derive(Debug, Clone, Deserialize)]
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
        match self.transport {
            McpTransportKind::Stdio if self.command.as_deref().unwrap_or("").trim().is_empty() => {
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
            _ => Ok(()),
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
}

#[derive(Debug, Clone)]
struct RpcResponse {
    result: Option<Value>,
    error: Option<RpcErrorObject>,
}

#[derive(Debug, Clone, Deserialize)]
struct RpcErrorObject {
    code: i64,
    message: String,
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
        let mut line = serde_json::to_vec(request)
            .map_err(|error| McpError::Protocol(format!("serialize request: {error}")))?;
        line.push(b'\n');
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
            })?;

        let mut response_line = String::new();
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
            break;
        }
        parse_json_rpc_response(response_line.as_bytes())
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
    session_id: Option<String>,
}

impl HttpTransport {
    fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let url = config
            .url
            .clone()
            .ok_or_else(|| McpError::Config("missing http url".into()))?;
        Ok(Self {
            client: reqwest::Client::new(),
            url,
            headers: config.headers.clone(),
            session_id: None,
        })
    }

    async fn request(&mut self, request: &RpcRequest<'_>) -> Result<RpcResponse, McpError> {
        let payload = serde_json::to_vec(request)
            .map_err(|error| McpError::Protocol(format!("serialize request: {error}")))?;
        let mut builder = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
        if let Some(session_id) = &self.session_id {
            builder = builder.header("Mcp-Session-Id", session_id);
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
        if let Some(session_id) = response.headers().get("Mcp-Session-Id") {
            self.session_id = Some(
                session_id
                    .to_str()
                    .map_err(|error| McpError::Protocol(format!("invalid MCP session header: {error}")))?
                    .to_owned(),
            );
        }
        if !response.status().is_success() {
            return Err(McpError::Transport {
                transport: "http".into(),
                message: format!("HTTP {}", response.status()),
            });
        }
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
        if content_type.contains("text/event-stream") {
            parse_sse_rpc_response(&body)
        } else {
            parse_json_rpc_response(&body)
        }
    }
}

#[derive(Debug)]
enum McpTransport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl McpTransport {
    async fn request(&mut self, request: &RpcRequest<'_>) -> Result<RpcResponse, McpError> {
        match self {
            Self::Stdio(transport) => transport.request(request).await,
            Self::Http(transport) => transport.request(request).await,
        }
    }
}

#[derive(Debug)]
struct McpServerClient {
    transport: McpTransport,
    next_id: u64,
}

impl McpServerClient {
    async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        config.validate()?;
        let transport = match config.transport {
            McpTransportKind::Stdio => {
                McpTransport::Stdio(StdioTransport::connect(&config).await?)
            }
            McpTransportKind::Http => McpTransport::Http(HttpTransport::connect(&config)?),
        };
        let mut client = Self { transport, next_id: 1 };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": CLIENT_NAME, "version": CLIENT_VERSION}
                }),
            )
            .await?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::Protocol("initialize response has no protocolVersion".into()))?;
        if protocol_version != MCP_PROTOCOL_VERSION && protocol_version != "2025-11-25" {
            return Err(McpError::Protocol(format!(
                "unsupported negotiated MCP protocol version: {protocol_version}"
            )));
        }
        self.notify("notifications/initialized", json!({})).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = RpcRequest { jsonrpc: "2.0", id, method, params };
        let response = self.transport.request(&request).await?;
        if let Some(error) = response.error {
            return Err(McpError::Remote { code: error.code, message: error.message });
        }
        response.result.ok_or_else(|| McpError::Protocol(format!("MCP response for '{method}' has no result")))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let payload = serde_json::to_vec(&json!({"jsonrpc":"2.0","method":method,"params":params}))
            .map_err(|error| McpError::Protocol(format!("serialize notification: {error}")))?;
        match &mut self.transport {
            McpTransport::Stdio(transport) => {
                let mut line = payload;
                line.push(b'\n');
                transport.stdin.write_all(&line).await.map_err(|error| McpError::Transport { transport: "stdio".into(), message: error.to_string() })?;
                transport.stdin.flush().await.map_err(|error| McpError::Transport { transport: "stdio".into(), message: error.to_string() })?;
                Ok(())
            }
            McpTransport::Http(transport) => {
                let mut builder = transport.client.post(&transport.url)
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
                if let Some(session_id) = &transport.session_id { builder = builder.header("Mcp-Session-Id", session_id); }
                for (name, value) in &transport.headers { builder = builder.header(name, value); }
                let response = builder.body(payload).send().await.map_err(|error| McpError::Transport { transport: "http".into(), message: error.to_string() })?;
                if response.status().is_success() { Ok(()) } else { Err(McpError::Transport { transport: "http".into(), message: format!("HTTP {}", response.status()) }) }
            }
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolDescriptor>, McpError> {
        let mut cursor = None;
        let mut tools = Vec::new();
        loop {
            let mut params = json!({"limit": 100});
            if let Some(cursor) = &cursor { params["cursor"] = Value::String(cursor.clone()); }
            let result = self.request("tools/list", params).await?;
            let page: ToolListPage = serde_json::from_value(result)
                .map_err(|error| McpError::Protocol(format!("invalid tools/list result: {error}")))?;
            tools.extend(page.tools);
            match page.next_cursor { Some(next) if !next.is_empty() => cursor = Some(next), _ => break }
        }
        Ok(tools)
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<ToolCallResult, McpError> {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments})).await?;
        serde_json::from_value(result).map_err(|error| McpError::Protocol(format!("invalid tools/call result: {error}")))
    }
}

#[derive(Debug, Deserialize)]
struct ToolListPage {
    #[serde(default)] tools: Vec<McpToolDescriptor>,
    #[serde(rename = "nextCursor", default)] next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallResult {
    #[serde(default)] content: Vec<Value>,
    #[serde(rename = "isError", default)] is_error: bool,
}

#[derive(Debug)]
struct McpBinding { server: String, tool: McpToolDescriptor }

#[derive(Debug)]
pub struct McpCatalog {
    clients: Mutex<HashMap<String, McpServerClient>>,
    bindings: Vec<McpBinding>,
}

impl McpCatalog {
    pub async fn from_env() -> Result<Self, McpError> {
        let raw = match std::env::var("GEMINI_ACP_MCP_SERVERS") { Ok(raw) if !raw.trim().is_empty() => raw, _ => return Ok(Self::empty()) };
        let configs: Vec<McpServerConfig> = serde_json::from_str(&raw)
            .map_err(|error| McpError::Config(format!("invalid GEMINI_ACP_MCP_SERVERS: {error}")))?;
        Self::connect(configs).await
    }

    pub async fn connect(configs: Vec<McpServerConfig>) -> Result<Self, McpError> {
        let mut clients = HashMap::new();
        let mut bindings = Vec::new();
        for config in configs {
            let server_name = config.name.clone();
            let mut client = McpServerClient::connect(config).await?;
            for tool in client.list_tools().await? {
                bindings.push(McpBinding { server: server_name.clone(), tool });
            }
            clients.insert(server_name, client);
        }
        Ok(Self { clients: Mutex::new(clients), bindings })
    }

    pub fn empty() -> Self {
        Self { clients: Mutex::new(HashMap::new()), bindings: Vec::new() }
    }

    pub fn has_tools(&self) -> bool { !self.bindings.is_empty() }

    pub fn definitions(&self) -> Vec<Value> {
        self.bindings.iter().map(|binding| json!({
            "name": qualified_name(&binding.server, &binding.tool.name),
            "description": binding.tool.description,
            "parameters": binding.tool.input_schema,
        })).collect()
    }

    pub async fn call_async(&self, qualified: &str, args: &Value, _cwd: &Path, _extra_dirs: &[PathBuf]) -> Option<super::registry::ToolResult> {
        let binding = self.bindings.iter().find(|binding| qualified_name(&binding.server, &binding.tool.name) == qualified)?;
        let mut clients = self.clients.lock().await;
        let client = clients.get_mut(&binding.server)?;
        match client.call_tool(&binding.tool.name, args.clone()).await {
            Ok(result) => {
                let rendered = result.content.iter().map(render_content).collect::<Vec<_>>().join("\n");
                if result.is_error { Some(super::registry::ToolResult::Err(rendered)) } else { Some(super::registry::ToolResult::Ok(rendered)) }
            }
            Err(error) => Some(super::registry::ToolResult::Err(error.to_string())),
        }
    }
}

fn qualified_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", sanitize_name(server), sanitize_name(tool))
}

fn sanitize_name(value: &str) -> String {
    value.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') { ch } else { '_' }).collect()
}

fn render_content(value: &Value) -> String {
    match value.get("type").and_then(Value::as_str) {
        Some("text") => value.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
        _ => value.to_string(),
    }
}

fn parse_json_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| McpError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    parse_json_rpc_value(value)
}

fn parse_sse_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    let text = std::str::from_utf8(bytes).map_err(|error| McpError::Protocol(format!("invalid SSE response: {error}")))?;
    let mut event_data = String::new();
    for line in text.lines() { if let Some(data) = line.strip_prefix("data:") { if !event_data.is_empty() { event_data.push('\n'); } event_data.push_str(data.trim_start()); } }
    if event_data.is_empty() { return Err(McpError::Protocol("SSE response contained no data event".into())); }
    parse_json_rpc_response(event_data.as_bytes())
}

fn parse_json_rpc_value(value: Value) -> Result<RpcResponse, McpError> {
    let object = value.as_object().ok_or_else(|| McpError::Protocol("JSON-RPC response is not an object".into()))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") { return Err(McpError::Protocol("JSON-RPC response has invalid jsonrpc version".into())); }
    Ok(RpcResponse {
        result: object.get("result").cloned(),
        error: object.get("error").cloned().map(serde_json::from_value).transpose().map_err(|error| McpError::Protocol(format!("invalid JSON-RPC error object: {error}")))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_names_are_collision_safe() {
        assert_eq!(qualified_name("foo-bar", "read_file"), "mcp__foo-bar__read_file");
        assert_eq!(qualified_name("foo/bar", "read file"), "mcp__foo_bar__read_file");
    }

    #[test]
    fn empty_catalog_has_no_tools() {
        let catalog = McpCatalog::empty();
        assert!(!catalog.has_tools());
        assert!(catalog.definitions().is_empty());
    }

    #[test]
    fn parses_json_rpc_error() {
        let response = parse_json_rpc_response(br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"not found"}}"#).expect("valid JSON-RPC");
        let error = response.error.expect("error object");
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "not found");
    }

    #[test]
    fn parses_sse_json_rpc_response() {
        let response = parse_sse_rpc_response(b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n").expect("valid SSE");
        assert!(response.result.is_some());
    }
}
