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
    llm_provider::core::models::resolve(requested, default)
}

pub fn usage(prompt: &str, completion: &str) -> Value {
    let pt = prompt.chars().count() / 4;
    let ct = completion.chars().count() / 4;
    json!({"prompt_tokens":pt,"completion_tokens":ct,"total_tokens":pt+ct})
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
    for cap in re.captures_iter(text) {
        let data: Value = match serde_json::from_str(cap[1].trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(name) = data.get("name").and_then(Value::as_str) else {
            continue;
        };
        let arguments = data.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let call_id = uuid::Uuid::new_v4().simple().to_string();
        tool_calls.push(json!({"id":format!("call_{call_id}"),"type":"function","function":{"name":name,"arguments":arguments.to_string()}}));
    }
    let clean = re.replace_all(text, "").trim().to_string();
    (clean, tool_calls)
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
        warn!(
            "xsrf_token configuré mais ignoré : le client gemini récupère SNlM0e automatiquement"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
