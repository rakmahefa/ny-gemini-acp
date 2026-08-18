use std::{
    collections::HashMap,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{MAX_MESSAGE_BYTES, MAX_PAGE_COUNT};

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
    pub(super) fn validate(&self) -> Result<(), McpError> {
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
            McpTransportKind::Stdio if self.command.as_deref().unwrap_or("").trim().is_empty() => {
                Err(McpError::Config(format!(
                    "stdio server '{}' is missing command",
                    self.name
                )))
            }
            McpTransportKind::Http if self.url.as_deref().unwrap_or("").trim().is_empty() => Err(
                McpError::Config(format!("http server '{}' is missing url", self.name)),
            ),
            _ => {
                super::protocol::validate_custom_headers(&self.headers)?;
                Ok(())
            }
        }
    }
}

impl From<agent_runtime::McpServerConfig> for McpServerConfig {
    fn from(config: agent_runtime::McpServerConfig) -> Self {
        Self {
            name: config.name,
            transport: match config.transport {
                agent_runtime::McpTransportKind::Stdio => McpTransportKind::Stdio,
                agent_runtime::McpTransportKind::Http => McpTransportKind::Http,
            },
            command: config.command,
            args: config.args,
            env: config.env,
            cwd: config.cwd,
            url: config.url,
            headers: config.headers,
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
    pub(super) fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() {
            return Err(McpError::Protocol(
                "MCP tool descriptor has empty name".into(),
            ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{McpServerConfig as RuntimeMcpServerConfig, McpTransportKind as RuntimeMcpTransportKind};

    #[test]
    fn tool_descriptor_requires_object_input_schema() {
        let descriptor = McpToolDescriptor {
            name: "x".into(),
            description: String::new(),
            input_schema: serde_json::json!([]),
            output_schema: None,
        };
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn converts_stdio_runtime_configuration() {
        let server = RuntimeMcpServerConfig::stdio(
            "project-tools",
            "/usr/local/bin/project-mcp",
            vec!["--cwd".into(), "/tmp/project".into()],
            HashMap::from([("TOKEN".into(), "secret".into())]),
            Some("/tmp/zed-workspace".into()),
        );
        let config = McpServerConfig::from(server);
        assert_eq!(config.name, "project-tools");
        assert_eq!(config.transport, McpTransportKind::Stdio);
        assert_eq!(config.command.as_deref(), Some("/usr/local/bin/project-mcp"));
        assert_eq!(config.args, ["--cwd", "/tmp/project"]);
        assert_eq!(config.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(config.cwd.as_deref(), Some(std::path::Path::new("/tmp/zed-workspace")));
    }

    #[test]
    fn converts_http_runtime_configuration() {
        let server = RuntimeMcpServerConfig::http(
            "remote",
            "https://mcp.example.test",
            HashMap::from([("authorization".into(), "Bearer test".into())]),
        );
        let config = McpServerConfig::from(server);
        assert_eq!(config.transport, McpTransportKind::Http);
        assert_eq!(config.url.as_deref(), Some("https://mcp.example.test"));
        assert_eq!(config.headers.get("authorization").map(String::as_str), Some("Bearer test"));
        assert_eq!(config.cwd, None);
    }

    #[test]
    fn conversion_preserves_transport_kind_contract() {
        assert_eq!(
            RuntimeMcpTransportKind::Stdio as u8,
            agent_runtime::McpTransportKind::Stdio as u8
        );
    }
}
