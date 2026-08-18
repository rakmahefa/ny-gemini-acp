use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::{
    config::{McpError, McpServerConfig, McpToolDescriptor, McpTransportKind},
    protocol::{
        legacy_initialize_params, legacy_initialized_notification, request_params, RpcRequest,
    },
    render::render_tool_content,
    transport::{HttpTransport, McpTransport, StdioTransport},
    CACHE_DEFAULT_TTL, MAX_PAGE_COUNT, REQUEST_TIMEOUT,
};

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
    legacy_mode: bool,
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
            legacy_mode: false,
        })
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    async fn request(
        &mut self,
        method: &str,
        tool_name: Option<&str>,
        params: Value,
    ) -> Result<Value, McpError> {
        let id = self.next_request_id();
        let request_params = if self.legacy_mode {
            params
        } else {
            request_params(params)
        };
        let request = RpcRequest::new(id, method, request_params);
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

    async fn initialize_legacy(&mut self) -> Result<(), McpError> {
        if self.legacy_mode {
            return Ok(());
        }

        let id = self.next_request_id();
        let initialize = RpcRequest::new(id, "initialize", legacy_initialize_params());
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            self.transport.request(&initialize, "initialize", None),
        )
        .await
        .map_err(|_| McpError::Transport {
            transport: "mcp".into(),
            message: "MCP initialize request timed out".into(),
        })??;

        if let Some(error) = response.error {
            return Err(McpError::Remote {
                code: error.code,
                message: error.message,
            });
        }
        let result = response.result.ok_or_else(|| {
            McpError::Protocol("MCP initialize response has no result".into())
        })?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if protocol_version.is_empty() {
            return Err(McpError::Protocol(
                "MCP initialize response has no protocolVersion".into(),
            ));
        }

        self.transport
            .notify("notifications/initialized", legacy_initialized_notification())
            .await?;
        self.legacy_mode = true;
        Ok(())
    }

    fn should_fallback_to_legacy(&self, error: &McpError) -> bool {
        if self.legacy_mode {
            return false;
        }
        if !matches!(self.transport, McpTransport::Stdio(_)) {
            return false;
        }
        matches!(
            error,
            McpError::Remote { code: -32601 | -32602, .. }
        )
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
            let mut params = json!({});
            if let Some(cursor) = &cursor {
                params["cursor"] = Value::String(cursor.clone());
            }
            let result = match self.request("tools/list", None, params.clone()).await {
                Ok(result) => result,
                Err(error) if self.should_fallback_to_legacy(&error) => {
                    tracing::debug!(%error, "MCP server rejected stateless tools/list; falling back to legacy initialize lifecycle");
                    self.initialize_legacy().await?;
                    self.request("tools/list", None, params).await?
                }
                Err(error) => return Err(error),
            };
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
}
