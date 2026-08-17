//! MCP tool infrastructure.
//!
//! This module deliberately keeps MCP behind the existing runtime tool surface.
//! Builtin tools remain native `Tool` implementations while MCP tools are
//! discovered dynamically and exposed through the same `ToolRegistry` API.

use std::{collections::HashMap, path::Path, path::PathBuf};

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
        let mut cursor: Option<String> = None;
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
    #[serde(default, rename = "isError")]
    is_error: bool,
}

#[derive(Debug)]
struct McpRegisteredTool {
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
        let mut servers = Vec::new();
        let mut tools = Vec::new();
        for config in configs {
            let server_name = config.name.clone();
            let mut client = McpServerClient::connect(config).await?;
            for descriptor in client.list_tools().await? {
                tools.push(McpRegisteredTool {
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
        self.tools.iter().map(|tool| {
            let name = format!("mcp__{}__{}", tool.server_name, tool.descriptor.name);
            json!({"name": name, "description": tool.descriptor.description, "parameters": tool.descriptor.input_schema})
        }).collect()
    }

    pub async fn call_async(
        &self,
        qualified_name: &str,
        args: &Value,
        _cwd: &Path,
        _extra_dirs: &[PathBuf],
    ) -> Option<crate::tools::registry::ToolResult> {
        let tool = self.tools.iter().find(|tool| {
            format!("mcp__{}__{}", tool.server_name, tool.descriptor.name) == qualified_name
        })?;
        let server_index = self.tools.iter().take_while(|candidate| !std::ptr::eq(*candidate, tool)).filter(|candidate| candidate.server_name == tool.server_name).count();
        let server = self.servers.get(server_index)?;
        let mut server = server.lock().await;
        let result = server.call_tool(&tool.descriptor.name, args.clone()).await;
        Some(match result {
            Ok(result) if result.is_error => crate::tools::registry::ToolResult::Err(render_tool_content(&result.content)),
            Ok(result) => crate::tools::registry::ToolResult::Ok(render_tool_content(&result.content)),
            Err(error) => crate::tools::registry::ToolResult::Err(error.to_string()),
        })
    }
}

fn render_tool_content(content: &[Value]) -> String {
    content.iter().map(|value| match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<unserializable MCP content>".into()),
    }).collect::<Vec<_>>().join("\n")
}

fn parse_json_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    parse_json_rpc_value(value)
}

fn parse_sse_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    for line in String::from_utf8_lossy(bytes).lines() {
        if let Some(data) = line.strip_prefix("data:") {
            if !data.trim().is_empty() {
                let value: Value = serde_json::from_str(data.trim())
                    .map_err(|error| McpError::Protocol(format!("invalid SSE JSON-RPC data: {error}")))?;
                return parse_json_rpc_value(value);
            }
        }
    }
    Err(McpError::Protocol("SSE response contained no data event".into()))
}

fn parse_json_rpc_value(value: Value) -> Result<RpcResponse, McpError> {
    let object = value.as_object().ok_or_else(|| McpError::Protocol("JSON-RPC response is not an object".into()))?;
    Ok(RpcResponse {
        result: object.get("result").cloned(),
        error: object.get("error").cloned().map(serde_json::from_value).transpose()
            .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC error: {error}")))?,
    })
}
