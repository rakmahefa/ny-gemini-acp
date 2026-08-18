use std::collections::HashMap;

use reqwest::header::CONTENT_TYPE;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use super::{
    protocol::{parse_json_rpc_response, parse_sse_rpc_response, serialize_request_line, serialize_request_payload, RpcRequest, RpcResponse},
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
