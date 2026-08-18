use std::{collections::HashMap, path::{Path, PathBuf}};

use agent_client_protocol::schema::v1::McpServer;
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
                super::protocol::validate_custom_headers(&self.headers)?;
                Ok(())
            }
        }
    }

    pub fn from_acp(server: McpServer, session_cwd: &Path) -> Result<Self, McpError> {
        match server {
            McpServer::Stdio(server) => {
                let command = server
                    .command
                    .to_str()
                    .ok_or_else(|| {
                        McpError::Config(format!(
                            "stdio MCP server '{}' command path is not valid UTF-8",
                            server.name
                        ))
                    })?
                    .to_owned();

                if command.trim().is_empty() {
                    return Err(McpError::Config(format!(
                        "stdio MCP server '{}' command is empty",
                        server.name
                    )));
                }
                if command.chars().any(|ch| ch == '\0' || ch.is_control()) {
                    return Err(McpError::Config(format!(
                        "stdio MCP server '{}' command contains control characters",
                        server.name
                    )));
                }

                let mut env = HashMap::new();
                for variable in server.env {
                    if variable.name.is_empty()
                        || variable.name.contains('=')
                        || variable.name.chars().any(|ch| ch == '\0' || ch.is_control())
                    {
                        return Err(McpError::Config(format!(
                            "stdio MCP server '{}' contains invalid environment variable name '{}'",
                            server.name, variable.name
                        )));
                    }
                    if env.insert(variable.name.clone(), variable.value).is_some() {
                        return Err(McpError::Config(format!(
                            "stdio MCP server '{}' contains duplicate environment variable '{}'",
                            server.name, variable.name
                        )));
                    }
                }
                Ok(Self {
                    name: server.name,
                    transport: McpTransportKind::Stdio,
                    command: Some(command),
                    args: server.args,
                    env,
                    cwd: Some(session_cwd.to_path_buf()),
                    url: None,
                    headers: HashMap::new(),
                })
            }
            McpServer::Http(server) => Ok(Self {
                name: server.name,
                transport: McpTransportKind::Http,
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                cwd: None,
                url: Some(server.url),
                headers: header_map(server.headers)?,
            }),
            McpServer::Sse(server) => Err(McpError::Config(format!(
                "MCP SSE transport for server '{}' is unsupported: the runtime requires MCP HTTP transport",
                server.name
            ))),
            _ => Err(McpError::Config(
                "unsupported MCP transport received from ACP client".into(),
            )),
        }
    }

    pub fn from_acp_servers(
        servers: Vec<McpServer>,
        session_cwd: &Path,
    ) -> Result<Vec<Self>, McpError> {
        servers
            .into_iter()
            .map(|server| Self::from_acp(server, session_cwd))
            .collect()
    }
}

fn header_map(
    headers: Vec<agent_client_protocol::schema::v1::HttpHeader>,
) -> Result<HashMap<String, String>, McpError> {
    let mut result: HashMap<String, String> = HashMap::with_capacity(headers.len());
    for header in headers {
        if result
            .keys()
            .any(|name: &String| name.eq_ignore_ascii_case(&header.name))
        {
            return Err(McpError::Config(format!(
                "duplicate MCP HTTP header '{}'",
                header.name
            )));
        }
        result.insert(header.name, header.value);
    }
    Ok(result)
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        EnvVariable, HttpHeader, McpServerHttp, McpServerSse, McpServerStdio,
    };

    fn cwd() -> &'static Path {
        Path::new("/tmp/zed-workspace")
    }

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
    fn forwards_stdio_configuration_without_losing_arguments_environment_or_cwd() {
        let server = McpServer::Stdio(
            McpServerStdio::new("project-tools", "/usr/local/bin/project-mcp")
                .args(vec!["--cwd".into(), "/tmp/project".into()])
                .env(vec![EnvVariable::new("TOKEN", "secret")]),
        );
        let config = McpServerConfig::from_acp(server, cwd()).unwrap();
        assert_eq!(config.name, "project-tools");
        assert_eq!(config.transport, McpTransportKind::Stdio);
        assert_eq!(config.command.as_deref(), Some("/usr/local/bin/project-mcp"));
        assert_eq!(config.args, ["--cwd", "/tmp/project"]);
        assert_eq!(config.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(config.cwd.as_deref(), Some(cwd()));
    }

    #[test]
    fn forwards_path_resolved_stdio_command() {
        let server = McpServer::Stdio(McpServerStdio::new("project-tools", "project-mcp"));
        let config = McpServerConfig::from_acp(server, cwd()).unwrap();
        assert_eq!(config.command.as_deref(), Some("project-mcp"));
        assert_eq!(config.cwd.as_deref(), Some(cwd()));
    }

    #[test]
    fn forwards_relative_stdio_command() {
        let server = McpServer::Stdio(McpServerStdio::new("project-tools", "./project-mcp"));
        let config = McpServerConfig::from_acp(server, cwd()).unwrap();
        assert_eq!(config.command.as_deref(), Some("./project-mcp"));
    }

    #[test]
    fn rejects_empty_stdio_command() {
        let server = McpServer::Stdio(McpServerStdio::new("project-tools", "   "));
        let error = McpServerConfig::from_acp(server, cwd()).unwrap_err();
        assert!(error.to_string().contains("command is empty"));
    }

    #[test]
    fn rejects_control_characters_in_stdio_command() {
        let server = McpServer::Stdio(McpServerStdio::new(
            "project-tools",
            "project-mcp\u{0007}",
        ));
        let error = McpServerConfig::from_acp(server, cwd()).unwrap_err();
        assert!(error.to_string().contains("control characters"));
    }

    #[test]
    fn forwards_http_headers_and_transport() {
        let server = McpServer::Http(
            McpServerHttp::new("remote", "https://mcp.example.test")
                .headers(vec![HttpHeader::new("authorization", "Bearer test")]),
        );
        let config = McpServerConfig::from_acp(server, cwd()).unwrap();
        assert_eq!(config.transport, McpTransportKind::Http);
        assert_eq!(config.url.as_deref(), Some("https://mcp.example.test"));
        assert_eq!(
            config.headers.get("authorization").map(String::as_str),
            Some("Bearer test")
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn legacy_sse_is_rejected_with_a_stable_configuration_error() {
        let server = McpServer::Sse(McpServerSse::new(
            "legacy-events",
            "https://mcp.example.test/events",
        ));
        let error = McpServerConfig::from_acp(server, cwd()).unwrap_err();
        assert!(error.to_string().contains("unsupported"));
    }

    #[test]
    fn duplicate_http_header_names_are_rejected_case_insensitively() {
        let server = McpServer::Http(
            McpServerHttp::new("remote", "https://mcp.example.test").headers(vec![
                HttpHeader::new("Authorization", "Bearer a"),
                HttpHeader::new("authorization", "Bearer b"),
            ]),
        );
        let error = McpServerConfig::from_acp(server, cwd()).unwrap_err();
        assert!(error.to_string().contains("duplicate MCP HTTP header"));
    }

    #[test]
    fn rejects_invalid_environment_variable_names() {
        let server = McpServer::Stdio(
            McpServerStdio::new("project-tools", "/usr/local/bin/project-mcp")
                .env(vec![EnvVariable::new("BAD=NAME", "secret")]),
        );
        let error = McpServerConfig::from_acp(server, cwd()).unwrap_err();
        assert!(error.to_string().contains("invalid environment variable name"));
    }
}
