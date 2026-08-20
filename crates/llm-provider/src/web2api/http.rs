//! Middleware CORS + auth par clé API et helpers JSON/SSE.

use super::config::Config;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAliveStream, Sse};
use axum::response::{IntoResponse, Response};
use llm_provider::client::Client;
use serde_json::Value;
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub type SseItem = Result<Event, Infallible>;
pub type SseChannel = (mpsc::Sender<SseItem>, mpsc::Receiver<SseItem>);
const MAX_BODY: usize = 16 * 1024 * 1024;
const CORS_ALLOW_HEADERS: &str = "Authorization, Content-Type, x-api-key, x-goog-api-key, Accept";

fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        let mut acc = 0xffu8;
        for byte in ab { acc ^= byte; }
        for byte in bb { acc ^= byte; }
        let _ = acc;
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) { acc |= x ^ y; }
    acc == 0
}

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub config: std::sync::Arc<Config>,
}
pub fn json_response(status: StatusCode, data: Value) -> Response {
    let body = serde_json::to_string(&data).unwrap_or_else(|_| "{}".into());
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}
pub fn json_ok(data: Value) -> Response { json_response(StatusCode::OK, data) }
pub fn sse_channel() -> SseChannel { mpsc::channel(16) }
pub fn sse_event(data: Value) -> Event {
    Event::default().data(serde_json::to_string(&data).unwrap_or_else(|_| "{}".into()))
}
pub fn sse(rx: mpsc::Receiver<SseItem>) -> Sse<KeepAliveStream<ReceiverStream<SseItem>>> {
    Sse::new(ReceiverStream::new(rx)).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(15)),
    )
}

pub async fn cors_auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let config = &state.config;
    if req.method() == Method::OPTIONS {
        return (
            StatusCode::NO_CONTENT,
            [
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS"),
                (header::ACCESS_CONTROL_ALLOW_HEADERS, CORS_ALLOW_HEADERS),
            ],
        ).into_response();
    }
    let path = req.uri().path().to_string();
    if !config.api_keys.is_empty() && path.starts_with("/v1") && !authorized(&req, config) {
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error":{"message":"invalid api key"}}));
    }
    let mut response = next.run(req).await;
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}
fn authorized(req: &Request, config: &Config) -> bool {
    if let Some(auth) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(key) = auth.strip_prefix("Bearer ") {
            if config.api_keys.iter().any(|k| constant_time_eq(k, key)) { return true; }
        }
    }
    for name in ["x-api-key", "x-goog-api-key"] {
        if let Some(v) = req.headers().get(name).and_then(|v| v.to_str().ok()) {
            if config.api_keys.iter().any(|k| constant_time_eq(k, v)) { return true; }
        }
    }
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(key) = pair.strip_prefix("key=") {
                if config.api_keys.iter().any(|k| constant_time_eq(k, key)) { return true; }
            }
        }
    }
    false
}
pub async fn json_body(req: Request) -> Result<Value, Response> {
    let bytes = match axum::body::to_bytes(req.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return Err(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error":{"message":"corps illisible"}}))),
    };
    serde_json::from_slice(&bytes).map_err(|_| json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error":{"message":"corps JSON invalide"}})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_time_eq_egalite() {
        assert!(constant_time_eq("sk-abc", "sk-abc"));
        assert!(constant_time_eq("", ""));
        assert!(constant_time_eq("a", "a"));
    }
    #[test]
    fn constant_time_eq_difference() {
        assert!(!constant_time_eq("sk-abc", "sk-abd"));
        assert!(!constant_time_eq("sk-abc", "sk-abcX"));
        assert!(!constant_time_eq("sk-abc", ""));
        assert!(!constant_time_eq("", "sk-abc"));
        assert!(!constant_time_eq("sk-abc", "sk-ABC"));
    }
}
