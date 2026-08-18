use std::{collections::HashMap, path::Path};

use agent_client_protocol::schema::v1::{HttpHeader, McpServer};
use agent_runtime::{McpServerConfig, McpTransportKind};

pub fn normalize_servers(
    servers: Vec<McpServer>,
    session_cwd: &Path,
) -> Result<Vec<McpServerConfig>, String> {
    servers
        .into_iter()
        .map(|server| normalize_server(server, session_cwd))
        .collect()
}

fn normalize_server(server: McpServer, session_cwd: &Path) -> Result<McpServerConfig, String> {
    match server {
        McpServer::Stdio(server) => {
            let command = server
                .command
                .to_str()
                .ok_or_else(|| format!("stdio MCP server '{}' command path is not valid UTF-8", server.name))?
                .to_owned();
            if command.trim().is_empty() {
                return Err(format!("stdio MCP server '{}' command is empty", server.name));
            }
            if command.chars().any(|ch| ch == '\0' || ch.is_control()) {
                return Err(format!(
                    "stdio MCP server '{}' command contains control characters",
                    server.name
                ));
            }

            let mut env = HashMap::new();
            for variable in server.env {
                if variable.name.is_empty()
                    || variable.name.contains('=')
                    || variable.name.chars().any(|ch| ch == '\0' || ch.is_control())
                {
                    return Err(format!(
                        "stdio MCP server '{}' contains invalid environment variable name '{}'",
                        server.name, variable.name
                    ));
                }
                if env.insert(variable.name.clone(), variable.value).is_some() {
                    return Err(format!(
                        "stdio MCP server '{}' contains duplicate environment variable '{}'",
                        server.name, variable.name
                    ));
                }
            }

            Ok(McpServerConfig::stdio(
                server.name,
                command,
                server.args,
                env,
                Some(session_cwd.to_path_buf()),
            ))
        }
        McpServer::Http(server) => Ok(McpServerConfig::http(
            server.name,
            server.url,
            header_map(server.headers)?,
        )),
        McpServer::Sse(server) => Err(format!(
            "MCP SSE transport for server '{}' is unsupported: the runtime requires MCP HTTP transport",
            server.name
        )),
        _ => Err("unsupported MCP transport received from ACP client".into()),
    }
}

fn header_map(headers: Vec<HttpHeader>) -> Result<HashMap<String, String>, String> {
    let mut result = HashMap::with_capacity(headers.len());
    for header in headers {
        if result
            .keys()
            .any(|name: &String| name.eq_ignore_ascii_case(&header.name))
        {
            return Err(format!("duplicate MCP HTTP header '{}'", header.name));
        }
        result.insert(header.name, header.value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{EnvVariable, HttpHeader, McpServerHttp, McpServerSse, McpServerStdio};

    #[test]
    fn normalizes_stdio_configuration() {
        let server = McpServer::Stdio(
            McpServerStdio::new("project-tools", "/usr/local/bin/project-mcp")
                .args(vec!["--cwd".into(), "/tmp/project".into()])
                .env(vec![EnvVariable::new("TOKEN", "secret")]),
        );
        let config = normalize_servers(vec![server], Path::new("/tmp/workspace")).unwrap();
        assert_eq!(config[0].transport, McpTransportKind::Stdio);
        assert_eq!(config[0].command.as_deref(), Some("/usr/local/bin/project-mcp"));
        assert_eq!(config[0].args, ["--cwd", "/tmp/project"]);
        assert_eq!(config[0].env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(config[0].cwd.as_deref(), Some(Path::new("/tmp/workspace")));
    }

    #[test]
    fn normalizes_http_configuration_and_headers() {
        let server = McpServer::Http(
            McpServerHttp::new("remote", "https://mcp.example.test")
                .headers(vec![HttpHeader::new("Authorization", "Bearer test")]),
        );
        let config = normalize_servers(vec![server], Path::new("/tmp/workspace")).unwrap();
        assert_eq!(config[0].transport, McpTransportKind::Http);
        assert_eq!(config[0].url.as_deref(), Some("https://mcp.example.test"));
        assert_eq!(config[0].headers.get("Authorization").map(String::as_str), Some("Bearer test"));
    }

    #[test]
    fn rejects_sse_and_duplicate_headers() {
        let sse = McpServer::Sse(McpServerSse::new(
            "legacy-events",
            "https://mcp.example.test/events",
        ));
        assert!(normalize_servers(vec![sse], Path::new("/tmp/workspace")).is_err());

        let duplicate = McpServer::Http(
            McpServerHttp::new("remote", "https://mcp.example.test").headers(vec![
                HttpHeader::new("Authorization", "a"),
                HttpHeader::new("authorization", "b"),
            ]),
        );
        assert!(normalize_servers(vec![duplicate], Path::new("/tmp/workspace")).is_err());
    }
}
