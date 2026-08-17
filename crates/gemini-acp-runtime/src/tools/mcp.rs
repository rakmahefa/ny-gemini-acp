//! MCP tool infrastructure.
//!
//! This module deliberately keeps MCP behind the existing runtime tool surface.
//! Builtin tools remain native `Tool` implementations while MCP tools are
//! discovered dynamically and exposed through the same `ToolRegistry` API.
//!
//! Supported transports:
//! - stdio child processes using line-delimited JSON-RPC;
//! - Streamable HTTP using JSON responses or SSE `data:` responses.
//!
//! Configuration is supplied through `GEMINI_ACP_MCP_SERVERS` as a JSON array:
//!
//! ```json
//! [
//!   {"name":"filesystem","transport":"stdio","command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/workspace"]},
//!   {"name":"remote","transport":"http","url":"http://127.0.0.1:3000/mcp"}
//! ]
//! ```
//!
//! MCP tools are exported under a collision-safe name:
//! `mcp__<server>__<tool>`.

use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
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
    #[error("MCP transport '{transport}' failed: {source}")]
    Transport { transport: String, source: String },
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

#[derive(Debug, Clone, Serialize)]
pub struct McpToolDescriptor {
    pub name: String,
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
            source: error.to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| McpError::Transport {
            transport: "stdio".into(),
            source: "child stdin unavailable".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| McpError::Transport {
            transport: "stdio".into(),
            source: "child stdout unavailable".into(),
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
                source: error.to_string(),
            })?;
        self.stdin
            .flush()
            .await
            .map_err(|error| McpError::Transport {
                transport: "stdio".into(),
                source: error.to_string(),
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
                    source: error.to_string(),
                })?;
            if read == 0 {
                let status = self.child.try_wait().ok().flatten();
                return Err(McpError::Transport {
                    transport: "stdio".into(),
                    source: format!("server closed stdout (status={status:?})"),
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
                source: error.to_string(),
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
                source: format!("HTTP {}", response.status()),
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
                source: error.to_string(),
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
    config: McpServerConfig,
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
        let mut client = Self {
            config,
            transport,
            next_id: 1,
        };
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
                    "clientInfo": {
                        "name": CLIENT_NAME,
                        "version": CLIENT_VERSION,
                    }
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
        let request = RpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let response = self.transport.request(&request).await?;
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

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        match &mut self.transport {
            // Notifications intentionally share the existing transport but do not
            // wait for a response. For stdio this is safe because initialize has
            // already drained the corresponding response.
            McpTransport::Stdio(transport) => {
                let request = json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                });
                let mut line = serde_json::to_vec(&request)
                    .map_err(|error| McpError::Protocol(format!("serialize notification: {error}")))?;
                line.push(b'\n');
                transport.stdin.write_all(&line).await.map_err(|error| McpError::Transport {
                    transport: "stdio".into(),
                    source: error.to_string(),
                })?;
                transport.stdin.flush().await.map_err(|error| McpError::Transport {
                    transport: "stdio".into(),
                    source: error.to_string(),
                })?;
                Ok(())
            }
            McpTransport::Http(transport) => {
                let payload = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                }))
                .map_err(|error| McpError::Protocol(format!("serialize notification: {error}")))?;
                let mut builder = transport
                    .client
                    .post(&transport.url)
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
                if let Some(session_id) = &transport.session_id {
                    builder = builder.header("Mcp-Session-Id", session_id);
                }
                for (name, value) in &transport.headers {
                    builder = builder.header(name, value);
                }
                let response = builder
                    .body(payload)
                    .send()
                    .await
                    .map_err(|error| McpError::Transport {
                        transport: "http".into(),
                        source: error.to_string(),
                    })?;
                if !response.status().is_success() {
                    return Err(McpError::Transport {
                        transport: "http".into(),
                        source: format!("HTTP {}", response.status()),
                    });
                }
                Ok(())
            }
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolDescriptor>, McpError> {
        let mut cursor: Option<String> = None;
        let mut tools = Vec::new();
        loop {
            let mut params = json!({});
            if let Some(value) = &cursor {
                params["cursor"] = Value::String(value.clone());
            }
            let page = self.request("tools/list", params).await?;
            let page_tools = page
                .get("tools")
                .and_then(Value::as_array)
                .ok_or_else(|| McpError::Protocol("tools/list response has no tools array".into()))?;
            for tool in page_tools {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| McpError::Protocol("MCP tool has no name".into()))?;
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("MCP tool")
                    .to_owned();
                let input_schema = tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                tools.push(McpToolDescriptor {
                    name: name.to_owned(),
                    description,
                    input_schema,
                });
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )
        .await
    }
}

#[derive(Debug)]
struct McpToolEntry {
    exported_name: String,
    server_name: String,
    remote_name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug)]
pub struct McpCatalog {
    clients: HashMap<String, Arc<Mutex<McpServerClient>>>,
    tools: HashMap<String, McpToolEntry>,
}

impl McpCatalog {
    pub async fn from_env() -> Result<Option<Self>, McpError> {
        let Some(raw) = std::env::var_os("GEMINI_ACP_MCP_SERVERS") else {
            return Ok(None);
        };
        let raw = raw.to_string_lossy();
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let configs: Vec<McpServerConfig> = serde_json::from_str(&raw)
            .map_err(|error| McpError::Config(format!("GEMINI_ACP_MCP_SERVERS: {error}")))?;
        Self::from_configs(configs).await.map(Some)
    }

    pub async fn from_configs(configs: Vec<McpServerConfig>) -> Result<Self, McpError> {
        let mut clients = HashMap::new();
        let mut tools = HashMap::new();
        for config in configs {
            if clients.contains_key(&config.name) {
                return Err(McpError::Config(format!(
                    "duplicate MCP server name: {}",
                    config.name
                )));
            }
            let server_name = config.name.clone();
            let client = McpServerClient::connect(config).await?;
            let client = Arc::new(Mutex::new(client));
            let descriptors = client.lock().await.list_tools().await?;
            for descriptor in descriptors {
                let exported_name = format!("mcp__{}__{}", sanitize_name(&server_name), sanitize_name(&descriptor.name));
                if tools.contains_key(&exported_name) {
                    return Err(McpError::Config(format!(
                        "MCP tool name collision: {}",
                        exported_name
                    )));
                }
                tools.insert(
                    exported_name.clone(),
                    McpToolEntry {
                        exported_name,
                        server_name: server_name.clone(),
                        remote_name: descriptor.name,
                        description: descriptor.description,
                        input_schema: descriptor.input_schema,
                    },
                );
            }
            clients.insert(server_name, client);
        }
        Ok(Self { clients, tools })
    }

    pub fn definitions(&self) -> Vec<Value> {
        let mut entries = self
            .tools
            .values()
            .map(|tool| {
                json!({
                    "name": tool.exported_name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                    "mcp": {
                        "server": tool.server_name,
                        "tool": tool.remote_name,
                    }
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.get("name")
                .and_then(Value::as_str)
                .cmp(&right.get("name").and_then(Value::as_str))
        });
        entries
    }

    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    pub async fn call_async(
        &self,
        name: &str,
        args: &Value,
        _cwd: &Path,
        _extra_dirs: &[PathBuf],
    ) -> Option<crate::tools::registry::ToolResult> {
        let entry = self.tools.get(name)?;
        let client = self.clients.get(&entry.server_name)?.clone();
        let mut client = client.lock().await;
        match client.call_tool(&entry.remote_name, args.clone()).await {
            Ok(result) => Some(crate::tools::registry::ToolResult::Ok(format_mcp_result(&result))),
            Err(error) => Some(crate::tools::registry::ToolResult::Err(error.to_string())),
        }
    }
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn format_mcp_result(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string()))
}

fn parse_json_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    parse_rpc_value(value)
}

fn parse_sse_rpc_response(bytes: &[u8]) -> Result<RpcResponse, McpError> {
    let text = String::from_utf8_lossy(bytes);
    let mut last_data = None;
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if !data.is_empty() {
                last_data = Some(data.to_owned());
            }
        }
    }
    let data = last_data.ok_or_else(|| McpError::Protocol("MCP SSE response has no data event".into()))?;
    parse_json_rpc_response(data.as_bytes())
}

fn parse_rpc_value(value: Value) -> Result<RpcResponse, McpError> {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(McpError::Protocol("response is not JSON-RPC 2.0".into()));
    }
    let error = value
        .get("error")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC error: {error}")))?;
    Ok(RpcResponse {
        result: value.get("result").cloned(),
        error,
    })
}

pub(crate) fn configured_servers_from_env() -> Result<Vec<McpServerConfig>, McpError> {
    let Some(raw) = std::env::var_os("GEMINI_ACP_MCP_SERVERS") else {
        return Ok(Vec::new());
    };
    let raw = raw.to_string_lossy();
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw)
        .map_err(|error| McpError::Config(format!("GEMINI_ACP_MCP_SERVERS: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_names_is_collision_safe_for_common_server_names() {
        assert_eq!(sanitize_name("filesystem@local"), "filesystem_local");
        assert_eq!(sanitize_name("foo-bar"), "foo-bar");
        assert_eq!(sanitize_name("foo.bar"), "foo.bar");
    }

    #[test]
    fn rpc_error_is_preserved() {
        let response = parse_json_rpc_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"bad args"}}"#,
        )
        .unwrap();
        let error = response.error.unwrap();
        assert_eq!(error.code, -32602);
        assert_eq!(error.message, "bad args");
    }

    #[test]
    fn sse_response_extracts_last_data_event() {
        let response = parse_sse_rpc_response(
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
        )
        .unwrap();
        assert_eq!(response.result.unwrap()["ok"], true);
    }

    #[test]
    fn mcp_names_are_qualified() {
        let exported = format!("mcp__{}__{}", sanitize_name("server"), sanitize_name("tool.name"));
        assert_eq!(exported, "mcp__server__tool.name");
    }
}
