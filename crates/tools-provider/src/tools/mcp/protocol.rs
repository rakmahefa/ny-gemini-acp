use std::collections::HashMap;

use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    McpError, CLIENT_NAME, CLIENT_VERSION, LEGACY_MCP_PROTOCOL_VERSION, MAX_MESSAGE_BYTES,
    MCP_PROTOCOL_VERSION, META_CLIENT_CAPABILITIES, META_CLIENT_INFO, META_PROTOCOL_VERSION,
};

#[derive(Debug, Clone)]
pub(super) struct RpcResponse {
    pub(super) result: Option<Value>,
    pub(super) error: Option<RpcErrorObject>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RpcErrorObject {
    pub(super) code: i64,
    pub(super) message: String,
}

#[derive(Debug, Clone)]
pub(super) struct RpcRequest<'a> {
    jsonrpc: &'static str,
    pub(super) id: u64,
    method: &'a str,
    pub(super) params: Value,
}

impl<'a> RpcRequest<'a> {
    pub(super) fn new(id: u64, method: &'a str, params: Value) -> Self {
        Self { jsonrpc: "2.0", id, method, params }
    }
}

impl<'a> Serialize for RpcRequest<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(&json!({
            "jsonrpc": self.jsonrpc,
            "id": self.id,
            "method": self.method,
            "params": self.params,
        }), serializer)
    }
}

pub(super) fn request_params(params: Value) -> Value {
    let mut params = match params { Value::Object(object) => object, _ => serde_json::Map::new() };
    params.insert("_meta".into(), json!({
        META_PROTOCOL_VERSION: MCP_PROTOCOL_VERSION,
        META_CLIENT_INFO: {"name": CLIENT_NAME, "version": CLIENT_VERSION},
        META_CLIENT_CAPABILITIES: {}
    }));
    Value::Object(params)
}

pub(super) fn legacy_initialize_params() -> Value {
    json!({
        "protocolVersion": LEGACY_MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": CLIENT_NAME, "version": CLIENT_VERSION}
    })
}

pub(super) fn legacy_initialized_notification() -> Value { json!({}) }

pub(super) fn serialize_request_payload(request: &RpcRequest<'_>) -> Result<Vec<u8>, McpError> {
    let payload = serde_json::to_vec(request).map_err(|error| McpError::Protocol(format!("serialize request: {error}")))?;
    if payload.len() > MAX_MESSAGE_BYTES { return Err(McpError::MessageTooLarge); }
    Ok(payload)
}

pub(super) fn serialize_request_line(request: &RpcRequest<'_>) -> Result<Vec<u8>, McpError> {
    let mut payload = serialize_request_payload(request)?;
    payload.push(b'\n');
    if payload.len() > MAX_MESSAGE_BYTES { return Err(McpError::MessageTooLarge); }
    Ok(payload)
}

pub(super) fn serialize_notification_line(method: &str, params: Value) -> Result<Vec<u8>, McpError> {
    let payload = serde_json::to_vec(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
        .map_err(|error| McpError::Protocol(format!("serialize notification: {error}")))?;
    let mut line = payload;
    line.push(b'\n');
    if line.len() > MAX_MESSAGE_BYTES { return Err(McpError::MessageTooLarge); }
    Ok(line)
}

pub(super) fn parse_json_rpc_response(bytes: &[u8], expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| McpError::Protocol(format!("invalid JSON-RPC response: {error}")))?;
    parse_json_rpc_value(value, expected_id)
}

pub(super) fn parse_sse_rpc_response(bytes: &[u8], expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let mut data_lines = Vec::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        if let Some(data) = line.strip_prefix("data:") { data_lines.push(data.trim_start().to_owned()); continue; }
        if line.trim().is_empty() && !data_lines.is_empty() { return parse_sse_data(&data_lines, expected_id); }
    }
    if !data_lines.is_empty() { return parse_sse_data(&data_lines, expected_id); }
    Err(McpError::Protocol("SSE response contained no data event".into()))
}

fn parse_sse_data(data_lines: &[String], expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let data = data_lines.join("\n");
    let value: Value = serde_json::from_str(&data).map_err(|error| McpError::Protocol(format!("invalid SSE JSON-RPC data: {error}")))?;
    parse_json_rpc_value(value, expected_id)
}

fn parse_json_rpc_value(value: Value, expected_id: Option<u64>) -> Result<RpcResponse, McpError> {
    let object = value.as_object().ok_or_else(|| McpError::Protocol("JSON-RPC response is not an object".into()))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") { return Err(McpError::Protocol("JSON-RPC response has invalid jsonrpc version".into())); }
    let id = object.get("id").cloned().ok_or_else(|| McpError::Protocol("JSON-RPC response has no id".into()))?;
    if let Some(expected_id) = expected_id {
        if id != Value::from(expected_id) {
            return Err(McpError::Protocol(format!("JSON-RPC response id mismatch: expected {expected_id}, got {id}")));
        }
    }
    let result = object.get("result").cloned();
    let error = object.get("error").cloned().map(serde_json::from_value).transpose()
        .map_err(|error| McpError::Protocol(format!("invalid JSON-RPC error object: {error}")))?;
    match (result.is_some(), error.is_some()) {
        (true, true) => Err(McpError::Protocol("JSON-RPC response contains both result and error".into())),
        (false, false) => Err(McpError::Protocol("JSON-RPC response contains neither result nor error".into())),
        _ => Ok(RpcResponse { result, error }),
    }
}

pub(super) fn validate_custom_headers(headers: &HashMap<String, String>) -> Result<(), McpError> {
    const RESERVED: &[&str] = &["content-type", "accept", "mcp-protocol-version", "mcp-method", "mcp-name", "mcp-session-id"];
    for (name, value) in headers {
        let parsed = HeaderName::from_bytes(name.as_bytes()).map_err(|error| McpError::Config(format!("invalid MCP HTTP header '{name}': {error}")))?;
        HeaderValue::from_str(value).map_err(|error| McpError::Config(format!("invalid value for MCP HTTP header '{name}': {error}")))?;
        if RESERVED.iter().any(|reserved| parsed.as_str().eq_ignore_ascii_case(reserved)) {
            return Err(McpError::Config(format!("MCP HTTP header '{}' is reserved and cannot be overridden", name)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_json_rpc_response_shape() {
        let err = parse_json_rpc_response(br#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-1,"message":"bad"}}"#, Some(1)).unwrap_err();
        assert!(err.to_string().contains("both result and error"));
    }

    #[test]
    fn rejects_response_id_mismatch() {
        let err = parse_json_rpc_response(br#"{"jsonrpc":"2.0","id":2,"result":{}}"#, Some(1)).unwrap_err();
        assert!(err.to_string().contains("id mismatch"));
    }

    #[test]
    fn legacy_initialize_payload_is_not_self_describing() {
        let request = RpcRequest::new(1, "initialize", legacy_initialize_params());
        let value: Value = serde_json::from_slice(&serialize_request_payload(&request).unwrap()).unwrap();
        assert_eq!(value["params"]["protocolVersion"], LEGACY_MCP_PROTOCOL_VERSION);
        assert!(value["params"].get("_meta").is_none());
    }

    #[test]
    fn request_payload_is_self_describing() {
        let request = RpcRequest::new(1, "tools/list", request_params(json!({})));
        let value: Value = serde_json::from_slice(&serialize_request_payload(&request).unwrap()).unwrap();
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["params"]["_meta"][META_PROTOCOL_VERSION], MCP_PROTOCOL_VERSION);
        assert_eq!(value["params"]["_meta"][META_CLIENT_INFO]["name"], CLIENT_NAME);
    }

    #[test]
    fn validates_reserved_http_headers() {
        let mut headers = HashMap::new();
        headers.insert("Mcp-Session-Id".into(), "forbidden".into());
        assert!(validate_custom_headers(&headers).is_err());
    }
}
