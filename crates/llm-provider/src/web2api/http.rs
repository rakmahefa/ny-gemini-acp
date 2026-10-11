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
        for byte in ab {
            acc ^= byte;
        }
        for byte in bb {
            acc ^= byte;
        }
        let _ = acc;
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        acc |= x ^ y;
    }
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
pub fn json_ok(data: Value) -> Response {
    json_response(StatusCode::OK, data)
}
pub fn sse_channel() -> SseChannel {
    mpsc::channel(16)
}
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
    let has_origin = req.headers().contains_key(header::ORIGIN);
    let origin = allowed_origin(&req, config);

    if has_origin && origin.is_none() {
        return json_response(
            StatusCode::FORBIDDEN,
            serde_json::json!({"error":{"message":"origin not allowed"}}),
        );
    }

    if req.method() == Method::OPTIONS {
        let Some(origin) = origin else {
            return StatusCode::FORBIDDEN.into_response();
        };
        return (
            StatusCode::NO_CONTENT,
            [
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, origin),
                (
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    HeaderValue::from_static("GET, POST, OPTIONS"),
                ),
                (
                    header::ACCESS_CONTROL_ALLOW_HEADERS,
                    HeaderValue::from_static(CORS_ALLOW_HEADERS),
                ),
                (header::VARY, HeaderValue::from_static("Origin")),
            ],
        )
            .into_response();
    }

    let path = req.uri().path();
    let mut response = if !config.api_keys.is_empty()
        && path.starts_with("/v1")
        && !authorized(&req, config)
    {
        json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error":{"message":"invalid api key"}}),
        )
    } else {
        next.run(req).await
    };

    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

fn allowed_origin(req: &Request, config: &Config) -> Option<HeaderValue> {
    let origin = req.headers().get(header::ORIGIN)?;
    let raw_origin = origin.to_str().ok()?;
    config
        .cors_origins
        .iter()
        .any(|allowed| allowed == raw_origin)
        .then(|| origin.clone())
}

fn authorized(req: &Request, config: &Config) -> bool {
    if let Some(auth) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(key) = auth.strip_prefix("Bearer ") {
            if config.api_keys.iter().any(|k| constant_time_eq(k, key)) {
                return true;
            }
        }
    }
    for name in ["x-api-key", "x-goog-api-key"] {
        if let Some(v) = req.headers().get(name).and_then(|v| v.to_str().ok()) {
            if config.api_keys.iter().any(|k| constant_time_eq(k, v)) {
                return true;
            }
        }
    }
    // Do not accept API keys in query strings: URLs can be logged, shared,
    // cached, or copied into browser history and referrers.
    false
}
pub async fn json_body(req: Request) -> Result<Value, Box<Response>> {
    let bytes = match axum::body::to_bytes(req.into_body(), MAX_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return Err(Box::new(json_response(
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error":{"message":"corps illisible"}}),
            )))
        }
    };
    serde_json::from_slice(&bytes).map_err(|_| {
        Box::new(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error":{"message":"corps JSON invalide"}}),
        ))
    })
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

    #[test]
    fn api_keys_are_accepted_only_from_headers() {
        let config = Config {
            api_keys: vec!["secret".into()],
            ..Config::default()
        };
        let query_request = Request::builder()
            .uri("/v1/models?key=secret")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(!authorized(&query_request, &config));

        let bearer_request = Request::builder()
            .uri("/v1/models")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(authorized(&bearer_request, &config));
    }

    #[test]
    fn cors_requires_an_exact_configured_origin() {
        let config = Config {
            cors_origins: vec!["http://localhost:3000".into()],
            ..Config::default()
        };
        let allowed = Request::builder()
            .uri("/v1/models")
            .header(header::ORIGIN, "http://localhost:3000")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            allowed_origin(&allowed, &config),
            Some(HeaderValue::from_static("http://localhost:3000"))
        );

        let untrusted = Request::builder()
            .uri("/v1/models")
            .header(header::ORIGIN, "https://example.invalid")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(allowed_origin(&untrusted, &config).is_none());
    }
}
