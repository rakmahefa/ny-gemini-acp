//! Helpers communs aux formats OpenAI / Codex / Google (refactor M9 §6.4).

use serde_json::{json, Value};
use tracing::warn;

/// Résout un nom de modèle strictement contre les modèles du provider Gemini.
pub fn resolve_model_strict(
    requested: &str,
    default: &str,
) -> Result<llm_provider::core::models::Resolved, String> {
    let base = requested.split("@think=").next().unwrap_or(requested);
    if !llm_provider::core::models::MODEL_KEYS.contains(&base) {
        return Err(format!("Unknown model: {requested}"));
    }
    llm_provider::core::models::resolve(requested, default).map_err(|e| e.to_string())
}

/// Estimation de tokens : partout en chars (C-15 — la sémantique divergente
/// chars/bytes donnait total ≠ input + output pour tout texte non-ASCII).
pub fn token_estimate(text: &str) -> u64 {
    (text.chars().count() / 4) as u64
}

pub fn usage(prompt: &str, completion: &str) -> Value {
    let pt = token_estimate(prompt);
    let ct = token_estimate(completion);
    json!({"prompt_tokens":pt,"completion_tokens":ct,"total_tokens":pt+ct})
}

/// Usage au format Responses API (`input_tokens` / `output_tokens`).
pub fn usage_responses(prompt: &str, completion: &str) -> Value {
    let pt = token_estimate(prompt);
    let ct = token_estimate(completion);
    json!({"input_tokens":pt,"output_tokens":ct,"total_tokens":pt+ct})
}

/// Usage au format Google GenerateContent (`usageMetadata`).
pub fn usage_google(prompt: &str, completion: &str) -> Value {
    let pt = token_estimate(prompt);
    let ct = token_estimate(completion);
    json!({"promptTokenCount":pt,"candidatesTokenCount":ct,"totalTokenCount":pt+ct})
}
pub fn tool_call_block(name: &str, args: &Value) -> String {
    format!(
        "```tool_call\n{}\n```",
        json!({"name":name,"arguments":args})
    )
}
pub fn parse_tool_calls(text: &str) -> (String, Vec<Value>) {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?s)```tool_call\s*\n(.*?)\n```").expect("regex statique")
    });
    let mut tool_calls = Vec::new();
    let mut removals: Vec<(usize, usize)> = Vec::new();
    for cap in re.captures_iter(text) {
        let whole = cap.get(0).expect("match has a whole span");
        // SPEC-P1-06 (M7): a block is removed from the response only when it
        // parses into a usable tool call. Invalid JSON stays visible and is
        // logged — silently deleting model output loses information.
        let data: Value = match serde_json::from_str(cap[1].trim()) {
            Ok(v) => v,
            Err(error) => {
                tracing::warn!(error = %error, "tool_call block with invalid JSON left in the response");
                continue;
            }
        };
        let Some(name) = data.get("name").and_then(Value::as_str) else {
            tracing::warn!("tool_call block without a tool name left in the response");
            continue;
        };
        let arguments = data.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let call_id = uuid::Uuid::new_v4().simple().to_string();
        tool_calls.push(json!({"id":format!("call_{call_id}"),"type":"function","function":{"name":name,"arguments":arguments.to_string()}}));
        removals.push((whole.start(), whole.end()));
    }
    let clean = remove_spans(text, &removals).trim().to_string();
    (clean, tool_calls)
}

/// Builds `text` without the given spans, assumed non-overlapping and in scan
/// order.
fn remove_spans(text: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for &(start, end) in spans {
        out.push_str(&text[cursor..start]);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}
impl ToolChoice {
    pub fn parse(v: Option<&Value>) -> Self {
        match v {
            Some(Value::String(s)) => match s.as_str() {
                "none" => Self::None,
                "required" => Self::Required,
                _ => Self::Auto,
            },
            Some(Value::Object(_)) => {
                let name = v
                    .and_then(|o| o.get("function"))
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name.is_empty() {
                    Self::Auto
                } else {
                    Self::Named(name.to_string())
                }
            }
            _ => Self::Auto,
        }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
    pub fn instruction(&self) -> String {
        match self {
            Self::Auto | Self::None => String::new(),
            Self::Required => {
                "\n\nIMPORTANT: You MUST call at least one tool. Do not respond with text only."
                    .into()
            }
            Self::Named(name) => format!(
                "\n\nIMPORTANT: You MUST call the tool \"{name}\". Do not call other tools."
            ),
        }
    }
}

pub fn warn_xsrf_ignored(xsrf: Option<&str>) {
    if xsrf.is_some() {
        warn!("xsrf_token configured but ignored: the gemini client fetches SNlM0e automatically");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_tool_call_json_stays_visible_in_the_response() {
        // SPEC-P1-06 (M7): an unparseable tool_call block must NOT be
        // silently removed from the response.
        let text = "avant\n```tool_call\n{invalid json}\n```\napres";
        let (clean, calls) = parse_tool_calls(text);
        assert!(calls.is_empty());
        assert!(clean.contains("{invalid json}"), "clean = {clean:?}");
        assert!(clean.contains("avant") && clean.contains("apres"));
    }

    #[test]
    fn parsed_tool_call_blocks_are_removed() {
        let text = "avant\n```tool_call\n{\"name\": \"a\", \"arguments\": {\"x\": 1}}\n```\napres";
        let (clean, calls) = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert!(clean.contains("avant") && clean.contains("apres"));
        assert!(!clean.contains("tool_call"), "clean = {clean:?}");
    }

    #[test]
    fn multi_line_tool_call_json_is_parsed_and_removed() {
        let text =
            "texte\n```tool_call\n{\n  \"name\": \"a\",\n  \"arguments\": {\"x\": 1}\n}\n```\nfin";
        let (clean, calls) = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "a");
        assert!(clean.contains("texte") && clean.contains("fin"));
    }

    #[test]
    fn tool_choice_parse_et_instruction() {
        assert_eq!(ToolChoice::parse(None), ToolChoice::Auto);
        assert_eq!(ToolChoice::parse(Some(&json!("none"))), ToolChoice::None);
        assert_eq!(
            ToolChoice::parse(Some(&json!("required"))),
            ToolChoice::Required
        );
        assert_eq!(ToolChoice::parse(Some(&json!("auto"))), ToolChoice::Auto);
        assert_eq!(
            ToolChoice::parse(Some(&json!({"type":"function","function":{"name":"lire"}}))),
            ToolChoice::Named("lire".into())
        );
        assert!(ToolChoice::Auto.instruction().is_empty());
        assert!(ToolChoice::Required
            .instruction()
            .contains("at least one tool"));
        assert!(ToolChoice::Named("lire".into())
            .instruction()
            .contains("lire"));
        assert!(ToolChoice::None.is_none());
        assert!(!ToolChoice::Auto.is_none());
    }
}
