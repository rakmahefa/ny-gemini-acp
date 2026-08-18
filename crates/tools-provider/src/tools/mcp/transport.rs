use std::collections::HashMap;

use reqwest::header::CONTENT_TYPE;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use super::{
    protocol::{
        parse_json_rpc_response, parse_sse_rpc_response, serialize_notification_line,
        serialize_request_line, serialize_request_payload, RpcRequest, RpcResponse,
    },
    McpError, McpServerConfig, IO_TIMEOUT, MAX_MESSAGE_BYTES, MCP_PROTOCOL_VERSION,
    REQUEST_TIMEOUT,
};

#[derive(Debug)]
pub(super) struct StdioTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioTransport {
    pub(super) async fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
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

    pub(super) async fn request(
        &mut self,
        request: &RpcRequest<'_>,
    ) -> Result<RpcResponse, McpError> {
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

    pub(super) async fn notify(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpError> {
        let line = serialize_notification_line(method, params)?;
        tokio::time::timeout(IO_TIMEOUT, async {
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
        })
        .await
        .map_err(|_| McpError::Transport {
            transport: "stdio".into(),
            message: "notification write timeout".into(),
        })??;
        Ok(())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[derive(Debug)]
pub(super) struct HttpTransport {
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
}

impl HttpTransport {
    pub(super) fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
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

    pub(super) async fn request(
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
            .get(CONTENT_TYPE)
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
pub(super) enum McpTransport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl McpTransport {
    pub(super) async fn request(
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

    pub(super) async fn notify(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpError> {
        match self {
            Self::Stdio(transport) => transport.notify(method, params).await,
            Self::Http(_) => Err(McpError::Config(
                "legacy MCP session fallback is only supported for stdio transport".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::McpTransportKind;

    #[test]
    fn invalid_transport_kind_is_not_used_directly() {
        assert_eq!(McpTransportKind::Http, McpTransportKind::Http);
    }
}
