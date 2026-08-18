//! Endpoints Google natifs (Gemini CLI).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::convert;
use super::http::{json_body, json_ok, json_response, sse, sse_channel, sse_event, AppState};
use crate::client::StreamItem;
use crate::core::models::{self, Resolved};

pub async fn models_list() -> Response {
    let model_names: Vec<Value> = models::MODEL_KEYS.iter().map(|name| serde_json::json!({"name": format!("models/{name}"), "displayName": name, "description": name, "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]})).collect();
    json_ok(serde_json::json!({ "models": model_names }))
}

pub async fn generate(State(state): State<AppState>, Path(model_path): Path<String>, req: axum::extract::Request) -> Response {
    let body = match json_body(req).await { Ok(b) => b, Err(e) => return e };
    let (model_name, stream) = if let Some(n) = model_path.strip_suffix(":streamGenerateContent") { (n.to_string(), true) } else if let Some(n) = model_path.strip_suffix(":streamGenerate") { (n.to_string(), true) } else if let Some(n) = model_path.strip_suffix(":generateContent") { (n.to_string(), false) } else { return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": {"message": "model not specified in path"}})); };
    let resolved = match convert::resolve_model_strict(&model_name, &state.config.default_model) { Ok(r) => r, Err(msg) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": {"message": msg}})) };
    let fc_mode = body.get("toolConfig").and_then(|c| c.get("functionCallingConfig")).and_then(|c| c.get("mode")).and_then(Value::as_str).unwrap_or("AUTO");
    let has_tools = body.get("tools").is_some() && fc_mode != "NONE";
    let (prompt, images) = convert::google_contents_to_prompt(&body);
    if prompt.trim().is_empty() { return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": {"message": "empty content"}})); }
    let mut refs = Vec::new();
    for (b64, mime) in images { match state.client.upload_image(&b64, &mime).await { Ok(reference) => refs.push(reference), Err(e) => tracing::warn!("upload d'image ignoré: {e:#}") } }
    if stream && !has_tools { return stream_chunks(&state, &prompt, &refs, &resolved, &model_name).await; }
    let text: String = match state.client.complete(&prompt, &resolved.name, Some(resolved.think), &refs).await { Ok(text) => text, Err(e) => return json_response(StatusCode::BAD_GATEWAY, serde_json::json!({"error": {"message": format!("upstream error: {e}")}})) };
    json_ok(response_object(&text, &model_name, prompt.len(), has_tools))
}

async fn stream_chunks(state: &AppState, prompt: &str, refs: &[String], resolved: &Resolved, model_name: &str) -> Response {
    let mut rx: mpsc::Receiver<StreamItem> = match state.client.stream(prompt, &resolved.name, Some(resolved.think), refs).await { Ok(rx) => rx, Err(e) => return json_response(StatusCode::BAD_GATEWAY, serde_json::json!({"error": {"message": format!("upstream error: {e}")}})) };
    let (tx, out) = sse_channel();
    let model_name = model_name.to_string(); let prompt = prompt.to_string();
    tokio::spawn(async move {
        let mut emitted = String::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(delta) => { emitted.push_str(delta.as_str()); let chunk = serde_json::json!({"candidates": [{"content": {"parts": [{"text": delta}], "role": "model"}, "index": 0}]}); if tx.send(Ok(sse_event(chunk))).await.is_err() { return; } }
                Err(error) => { tracing::warn!("stream generateContent interrompu: {error}"); break; }
            }
        }
        let final_chunk = serde_json::json!({"candidates": [{"content": {"parts": [{"text": ""}], "role": "model"}, "finishReason": "STOP", "index": 0}], "usageMetadata": {"promptTokenCount": prompt.chars().count() / 4, "candidatesTokenCount": emitted.chars().count() / 4, "totalTokenCount": (prompt.len() + emitted.len()) / 4}, "modelVersion": model_name});
        let _ = tx.send(Ok(sse_event(final_chunk))).await;
    });
    sse(out).into_response()
}

fn response_object(text: &str, model_name: &str, prompt_len: usize, has_tools: bool) -> Value {
    let parts: Vec<Value> = if has_tools {
        let (clean, calls) = convert::parse_google_function_calls(text);
        let mut parts = Vec::new();
        if !clean.is_empty() { parts.push(json!({ "text": clean })); }
        for fc in calls { parts.push(json!({"functionCall": {"name": fc.get("name"), "args": fc.get("args")}})); }
        if parts.is_empty() { parts.push(json!({ "text": text })); }
        parts
    } else { vec![json!({ "text": text })] };
    let candidate = serde_json::json!({"content": {"parts": parts, "role": "model"}, "finishReason": "STOP", "index": 0});
    let usage = serde_json::json!({"promptTokenCount": prompt_len / 4, "candidatesTokenCount": text.len() / 4, "totalTokenCount": (prompt_len + text.len()) / 4});
    serde_json::json!({"candidates": [candidate], "usageMetadata": usage, "modelVersion": model_name})
}
