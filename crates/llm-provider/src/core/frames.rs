//! Décodage défensif du flux `StreamGenerate` Gemini Web.
//!
//! Pipeline:
//!
//! ```text
//! GEMINI RAW
//!    │
//!    ▼
//! GeminiFrameDecoder
//!    ├── Text
//!    ├── ToolCall
//!    └── Metadata
//! ```
//!
//! Le décodeur ne transforme plus prématurément toute la réponse en `String`.
//! Les structures tool/function présentes dans les frames JSON sont préservées
//! jusqu'à `GeminiSemanticStream`. Le parsing de marqueurs textuels reste un
//! fallback séparé dans `semantic_stream::protocol`.

use anyhow::{bail, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOOL_EVENTS_PER_STREAM: usize = 256;
const MAX_METADATA_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum GeminiFrameEvent {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    Metadata {
        kind: String,
        value: Value,
    },
}

#[derive(Debug)]
pub struct GeminiFrameDecoder {
    buf: String,
    emitted_tool_ids: HashSet<String>,
    next_call_id: usize,
}

impl Default for GeminiFrameDecoder {
    fn default() -> Self {
        Self {
            buf: String::new(),
            emitted_tool_ids: HashSet::new(),
            next_call_id: 0,
        }
    }
}

impl GeminiFrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> &str {
        &self.buf
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.emitted_tool_ids.clear();
        self.next_call_id = 0;
    }

    pub fn feed(&mut self, chunk: &str) -> Vec<GeminiFrameEvent> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.buf.push_str(chunk);
        if self.buf.len() > MAX_BUFFER_BYTES && !self.buf.contains('\n') {
            tracing::warn!(
                bytes = self.buf.len(),
                "GeminiFrameDecoder: oversized unterminated frame; purging buffer"
            );
            self.buf.clear();
            return Vec::new();
        }

        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let remainder = self.buf.split_off(pos + 1);
            let line = std::mem::replace(&mut self.buf, remainder);
            out.extend(self.decode_line(line.trim_end_matches(['\r', '\n'])));
        }
        out
    }

    pub fn finish(&mut self) -> Vec<GeminiFrameEvent> {
        if self.buf.trim().is_empty() {
            self.buf.clear();
            return Vec::new();
        }
        let line = std::mem::take(&mut self.buf);
        self.decode_line(line.trim_end_matches(['\r', '\n']))
    }

    fn decode_line(&mut self, line: &str) -> Vec<GeminiFrameEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == ")]}'" {
            return Vec::new();
        }

        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            // Non-JSON lines are expected around the wire framing. Do not guess
            // their semantics here; the raw stream remains observable in traces.
            tracing::trace!(bytes = trimmed.len(), "GeminiFrameDecoder ignored non-JSON line");
            return Vec::new();
        };

        let Some(inner_str) = extract_wrapped_inner(&value) else {
            return Vec::new();
        };
        let Ok(inner) = serde_json::from_str::<Value>(inner_str) else {
            tracing::debug!("GeminiFrameDecoder: wrb.fr inner payload is not valid JSON");
            return vec![metadata("unparsed_frame", bounded_json(inner_str))];
        };

        let mut events = Vec::new();
        let mut tools = Vec::new();
        collect_tool_calls(&inner, &mut tools);

        for tool in tools {
            if self.emitted_tool_ids.len() >= MAX_TOOL_EVENTS_PER_STREAM {
                tracing::warn!("GeminiFrameDecoder: tool event limit reached");
                break;
            }
            let id = tool
                .id
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| self.allocate_call_id());
            let identity = id.clone();
            if !self.emitted_tool_ids.insert(identity) {
                continue;
            }
            events.push(GeminiFrameEvent::ToolCall {
                id,
                name: tool.name,
                arguments: tool.arguments,
            });
        }

        if let Some(text) = longest_candidate_text(&inner) {
            events.insert(0, GeminiFrameEvent::Text(text));
        }

        collect_metadata(&inner, &mut events);
        events
    }

    fn allocate_call_id(&mut self) -> String {
        let id = format!("gemini_call_{}", self.next_call_id);
        self.next_call_id = self.next_call_id.saturating_add(1);
        id
    }
}

#[derive(Debug)]
struct ParsedToolCall {
    id: Option<String>,
    name: String,
    arguments: Value,
}

fn extract_wrapped_inner(value: &Value) -> Option<&str> {
    if !value.is_array() {
        return None;
    }
    let first = value.get(0)?;
    if first.get(0).and_then(Value::as_str) != Some("wrb.fr") {
        return None;
    }
    first.get(2).and_then(Value::as_str)
}

fn longest_candidate_text(inner: &Value) -> Option<String> {
    let candidates = inner.get(4)?.as_array()?;
    candidates
        .iter()
        .filter_map(|part| {
            let segments = part.get(1)?.as_array()?;
            let text: String = segments.iter().filter_map(Value::as_str).collect();
            (!text.is_empty()).then_some(text)
        })
        .max_by_key(String::len)
}

fn collect_tool_calls(value: &Value, out: &mut Vec<ParsedToolCall>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_tool_calls(value, out);
            }
        }
        Value::Object(map) => {
            for key in ["toolCall", "functionCall", "tool_call", "function_call"] {
                if let Some(candidate) = map.get(key) {
                    if let Some(parsed) = parse_tool_object(candidate) {
                        out.push(parsed);
                    } else {
                        collect_tool_calls(candidate, out);
                    }
                }
            }

            if let Some(parsed) = parse_tool_object(value) {
                out.push(parsed);
                return;
            }

            for child in map.values() {
                collect_tool_calls(child, out);
            }
        }
        Value::String(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                collect_tool_calls(&value, out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn parse_tool_object(value: &Value) -> Option<ParsedToolCall> {
    let map = value.as_object()?;
    let name = map
        .get("name")
        .or_else(|| map.get("functionName"))
        .or_else(|| map.get("toolName"))
        .and_then(Value::as_str)?
        .trim();
    if name.is_empty() {
        return None;
    }

    let arguments = map
        .get("arguments")
        .or_else(|| map.get("args"))
        .or_else(|| map.get("parameters"))
        .cloned()?;

    let id = map
        .get("id")
        .or_else(|| map.get("callId"))
        .or_else(|| map.get("call_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    Some(ParsedToolCall {
        id,
        name: name.to_owned(),
        arguments,
    })
}

fn collect_metadata(inner: &Value, events: &mut Vec<GeminiFrameEvent>) {
    let map = match inner.as_object() {
        Some(map) => map,
        None => return,
    };
    for key in ["usageMetadata", "usage", "finishReason", "blockReason"] {
        if let Some(value) = map.get(key) {
            let bounded = serde_json::to_vec(value)
                .ok()
                .filter(|bytes| bytes.len() <= MAX_METADATA_BYTES)
                .and_then(|_| Some(value.clone()));
            if let Some(value) = bounded {
                events.push(metadata(key, value));
            } else {
                tracing::warn!(kind = key, "GeminiFrameDecoder dropped oversized metadata");
            }
        }
    }
}

fn metadata(kind: impl Into<String>, value: Value) -> GeminiFrameEvent {
    GeminiFrameEvent::Metadata {
        kind: kind.into(),
        value,
    }
}

fn bounded_json(raw: &str) -> Value {
    serde_json::json!({
        "bytes": raw.len(),
        "preview": raw.chars().take(512).collect::<String>()
    })
}

fn code_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)```(?:python|javascript|text)\?code_(?:reference|stdout)&code_event_index=\d+\n.*?```\n?")
            .expect("regex code_ref")
    })
}

fn card_content_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"http://googleusercontent\.com/card_content/\d+\n?").expect("regex card"))
}

pub fn clean_text(text: &str, strip: bool) -> String {
    let out = code_ref_re().replace_all(text, "");
    let out = card_content_re().replace_all(&out, "").into_owned();
    if strip { out.trim().to_string() } else { out }
}

pub fn bard_error(raw: &str) -> Option<i64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"BardErrorInfo\s*\[(\d+)\]").expect("regex bard"));
    re.captures(raw)?.get(1)?.as_str().parse().ok()
}

pub fn detect_safety_block(raw: &str) -> Option<String> {
    if raw.contains("blockReason") {
        let start = raw.find(r#"\"blockReason\":\"#).or_else(|| raw.find(r#"\"blockReason\": \"#));
        if let Some(start) = start {
            let after_colon = &raw[start..];
            let colon_pos = after_colon.find(':').unwrap_or(0);
            let rest = after_colon[colon_pos + 1..].trim_start();
            if let Some(end) = rest.find('"') {
                let reason = &rest[..end];
                if !reason.is_empty() {
                    return Some(format!("Gemini a refusé de répondre (blockReason: {reason}). Reformulez votre prompt."));
                }
            }
        }
        return Some("Gemini a refusé de répondre (politique de sécurité). Reformulez votre prompt.".to_string());
    }
    let safety_phrases = [
        "I can't help with that",
        "I'm not able to help with that",
        "I cannot fulfill this request",
        "I won't be able to help",
        "content safety",
        "against my safety guidelines",
        "violates safety policy",
    ];
    let lower = raw.to_lowercase();
    safety_phrases
        .iter()
        .find(|phrase| lower.contains(&phrase.to_lowercase()))
        .map(|_| "Gemini a refusé de répondre à ce prompt (politique de contenu). Reformulez votre demande.".to_string())
}

pub fn is_empty_stream(raw: &str) -> bool {
    if !raw.contains("\"wrb.fr\"") {
        return false;
    }
    let mut decoder = GeminiFrameDecoder::new();
    decoder.feed(raw).into_iter().all(|event| !matches!(event, GeminiFrameEvent::Text(ref text) if !text.is_empty()))
}

pub fn final_text(raw: &str) -> Result<String> {
    if let Some(code) = bard_error(raw) {
        bail!("Gemini upstream rejected request: BardErrorInfo [{code}]");
    }
    let mut decoder = GeminiFrameDecoder::new();
    let text = decoder
        .feed(raw)
        .into_iter()
        .chain(decoder.finish())
        .filter_map(|event| match event { GeminiFrameEvent::Text(text) => Some(text), _ => None })
        .max_by_key(String::len)
        .unwrap_or_default();
    Ok(clean_text(&text, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire_line(inner: Value) -> String {
        let escaped = serde_json::to_string(&inner.to_string()).unwrap();
        format!("[[\"wrb.fr\",[62,0],{escaped}],[\"di\",72]]")
    }

    fn inner_with_candidates(candidates: Value) -> Value {
        json!([null, ["tok"], "padding-padding-padding-padding-padding-padding-padding-padding-padding-padding-padding-padding-padding-padding-padding", [], candidates, [], [], []])
    }

    #[test]
    fn structured_tool_call_is_preserved() {
        let inner = json!({
            "candidates": [["ignored"]],
            "functionCall": {"id": "c1", "name": "glob", "arguments": {"pattern": "*.rs"}}
        });
        let line = wire_line(inner);
        let events = GeminiFrameDecoder::new().feed(&(line + "\n"));
        assert!(events.iter().any(|event| matches!(event, GeminiFrameEvent::ToolCall { id, name, arguments } if id == "c1" && name == "glob" && arguments["pattern"] == "*.rs")));
    }

    #[test]
    fn duplicate_structured_tool_call_is_emitted_once() {
        let inner = json!({"toolCall": {"id": "c1", "name": "glob", "arguments": {}}});
        let line = wire_line(inner);
        let mut decoder = GeminiFrameDecoder::new();
        let first = decoder.feed(&(line.clone() + "\n"));
        let second = decoder.feed(&(line + "\n"));
        assert_eq!(first.iter().filter(|event| matches!(event, GeminiFrameEvent::ToolCall { .. })).count(), 1);
        assert_eq!(second.iter().filter(|event| matches!(event, GeminiFrameEvent::ToolCall { .. })).count(), 0);
    }

    #[test]
    fn longest_candidate_is_selected() {
        let inner = inner_with_candidates(json!([
            ["short", ["Bonjour"]],
            ["long", ["Bonjour, ", "le monde"]]
        ]));
        let line = wire_line(inner);
        let events = GeminiFrameDecoder::new().feed(&(line + "\n"));
        assert!(events.iter().any(|event| matches!(event, GeminiFrameEvent::Text(text) if text == "Bonjour, le monde")));
    }

    #[test]
    fn final_partial_line_is_flushed() {
        let inner = inner_with_candidates(json!([["c", ["abc"]]]));
        let line = wire_line(inner);
        let mut decoder = GeminiFrameDecoder::new();
        assert!(decoder.feed(&line).is_empty());
        let events = decoder.finish();
        assert!(events.iter().any(|event| matches!(event, GeminiFrameEvent::Text(text) if text == "abc")));
    }

    #[test]
    fn malformed_inner_becomes_bounded_metadata() {
        let line = "[[\"wrb.fr\",[62,0],\"{not-json\"],[\"di\",72]]\n";
        let events = GeminiFrameDecoder::new().feed(line);
        assert!(events.iter().any(|event| matches!(event, GeminiFrameEvent::Metadata { kind, .. } if kind == "unparsed_frame")));
    }

    #[test]
    fn clean_text_enleve_references_et_cards() {
        let input = "avant\n```python?code_reference&code_event_index=12\nligne 1\nligne 2\n```\nmilieu\nhttp://googleusercontent.com/card_content/7\nfin\n";
        assert_eq!(clean_text(input, true), "avant\nmilieu\nfin");
    }

    #[test]
    fn final_text_uses_decoder() {
        let raw = format!(")]}\'\n{}\n", wire_line(inner_with_candidates(json!([["c", ["court"]]]))));
        assert_eq!(final_text(&raw).unwrap(), "court");
    }

    #[test]
    fn final_text_bard_error() {
        let raw = ")]}\' foo\nBardErrorInfo [123] bar";
        assert!(final_text(raw).unwrap_err().to_string().contains("BardErrorInfo [123]"));
    }
}
