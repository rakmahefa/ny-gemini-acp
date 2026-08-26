//! Parsing normalisé des appels d'outils depuis les réponses texte Gemini.
//!
//! Les appels ordinaires sont des `Tool`; FollowUp est explicitement classé
//! comme `Action` afin qu'aucune couche d'exécution générique ne le traite
//! comme un outil exécutable.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedToolKind {
    Tool,
    Elicitation,
    Action,
}
impl ParsedToolKind {
    pub fn is_elicitation(self) -> bool {
        matches!(self, Self::Elicitation)
    }
    pub fn is_action(self) -> bool {
        matches!(self, Self::Action)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub kind: ParsedToolKind,
}
impl ParsedToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        let original_name = name.into();
        let kind = classify_tool_kind(&original_name);
        let name = if kind.is_elicitation() {
            "AskUserQuestion".to_owned()
        } else {
            original_name
        };
        Self {
            id: id.into(),
            name,
            arguments,
            kind,
        }
    }
    pub fn is_elicitation(&self) -> bool {
        self.kind.is_elicitation()
    }
    pub fn is_action(&self) -> bool {
        self.kind.is_action()
    }
    pub fn to_history_block(&self) -> String {
        format!(
            "```tool_call\n{}\n```",
            json!({"id": self.id, "name": self.name, "arguments": self.arguments})
        )
    }
}

fn classify_tool_kind(name: &str) -> ParsedToolKind {
    let normalized = name.trim().to_ascii_lowercase().replace(['-', '_'], "");
    match normalized.as_str() {
        "askuserquestion" | "elicitation" | "askuser" => ParsedToolKind::Elicitation,
        "followup" => ParsedToolKind::Action,
        _ => ParsedToolKind::Tool,
    }
}
fn generated_id(sequence: usize) -> String {
    format!("gemini_call_{sequence}")
}

/// Removes protocol-role markers that Gemini occasionally echoes after a tool round.
/// This is deliberately conservative: only a leading tool-result envelope followed
/// by an assistant envelope is discarded, and a standalone leading assistant marker
/// is stripped. Tool output itself is never executed or reinterpreted here.
pub fn sanitize_assistant_text(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(assistant_pos) = trimmed.find("[Assistant]:") {
        let prefix = &trimmed[..assistant_pos];
        if prefix.contains("[Tool result for ") {
            return trimmed[assistant_pos + "[Assistant]:".len()..]
                .trim()
                .to_owned();
        }
    }
    trimmed
        .strip_prefix("[Assistant]:")
        .map(str::trim)
        .unwrap_or(trimmed)
        .to_owned()
}

pub fn parse_tool_calls(text: &str) -> (String, Vec<ParsedToolCall>) {
    static RE_TOOL_CALL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static RE_FUNC_CALL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re_tool_call = RE_TOOL_CALL.get_or_init(|| {
        regex::Regex::new(r"(?s)```tool_call\s*\n(.*?)\n```").expect("regex statique")
    });
    let re_func_call = RE_FUNC_CALL.get_or_init(|| {
        regex::Regex::new(r"(?s)```function_call\s*\n(.*?)\n```").expect("regex statique")
    });
    let mut calls = Vec::new();
    let mut clean = text.to_string();
    for cap in re_tool_call.captures_iter(&clean) {
        if let Some(call) = parse_single(cap[1].trim(), calls.len()) {
            calls.push(call);
        }
    }
    clean = re_tool_call.replace_all(&clean, "").trim().to_string();
    for cap in re_func_call.captures_iter(&clean) {
        if let Some(call) = parse_single_func(cap[1].trim(), calls.len()) {
            calls.push(call);
        }
    }
    clean = re_func_call.replace_all(&clean, "").trim().to_string();
    let (without_follow_up, follow_up) = extract_follow_up(&clean);
    clean = without_follow_up;
    if let Some((label, query)) = follow_up {
        calls.push(ParsedToolCall::new(
            generated_id(calls.len()),
            "FollowUp",
            json!({"label": label, "query": query}),
        ));
    }
    if calls.is_empty() && clean.trim_start().starts_with('{') {
        if let Ok(data) = serde_json::from_str::<Value>(clean.trim()) {
            if let Some(name) = data.get("name").and_then(Value::as_str) {
                let args = data
                    .get("arguments")
                    .or_else(|| data.get("args"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                calls.push(ParsedToolCall::new(generated_id(0), name, args));
                clean.clear();
            }
        }
    }
    (clean, calls)
}

fn extract_follow_up(text: &str) -> (String, Option<(String, String)>) {
    const MARKER: &str = "<FollowUp";
    let mut clean = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut found = None;
    while let Some(relative_start) = text[cursor..].find(MARKER) {
        let start = cursor + relative_start;
        clean.push_str(&text[cursor..start]);
        let Some(relative_end) = find_tag_end(&text[start + MARKER.len()..]) else {
            clean.push_str(&text[start..]);
            cursor = text.len();
            break;
        };
        let end = start + MARKER.len() + relative_end;
        let tag = &text[start..=end];
        if let Some((label, query)) = parse_follow_up_tag(tag) {
            if found.is_none() {
                found = Some((label, query));
            }
        } else {
            clean.push_str(tag);
        }
        cursor = end + 1;
    }
    if found.is_some() && clean.ends_with('\n') && text[cursor..].starts_with('\n') {
        cursor += 1;
    }
    clean.push_str(&text[cursor..]);
    (clean.trim().to_owned(), found)
}
fn find_tag_end(input: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in input.as_bytes().iter().copied().enumerate() {
        match quote {
            Some(current) if byte == current => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}
fn parse_follow_up_tag(tag: &str) -> Option<(String, String)> {
    let inner = tag.strip_prefix("<FollowUp")?.strip_suffix('>')?.trim();
    let inner = inner.strip_suffix('/').unwrap_or(inner).trim();
    let attrs = agent_runtime::text::parse_tag_attributes(inner);
    let label = attrs.get("label")?.trim();
    let query = attrs.get("query")?.trim();
    if label.is_empty() || query.is_empty() {
        return None;
    }
    Some((decode_xml(label), decode_xml(query)))
}
fn decode_xml(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
fn parse_single(raw: &str, sequence: usize) -> Option<ParsedToolCall> {
    let data: Value = serde_json::from_str(raw).ok()?;
    let name = data.get("name").and_then(Value::as_str)?;
    let id = data
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| generated_id(sequence));
    let arguments = data.get("arguments").cloned().unwrap_or_else(|| json!({}));
    Some(ParsedToolCall::new(id, name, arguments))
}
fn parse_single_func(raw: &str, sequence: usize) -> Option<ParsedToolCall> {
    let data: Value = serde_json::from_str(raw).ok()?;
    let name = data.get("name").and_then(Value::as_str)?;
    let id = data
        .get("id")
        .or_else(|| data.get("call_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| generated_id(sequence));
    let arguments = data
        .get("args")
        .or_else(|| data.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    Some(ParsedToolCall::new(id, name, arguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitize_echoed_tool_result_and_assistant_marker() {
        let s = "[Tool result for shell_exec]: Finished `dev` profile\n\n[Assistant]: J'exécute `cargo check`";
        assert_eq!(sanitize_assistant_text(s), "J'exécute `cargo check`");
    }
    #[test]
    fn sanitize_standalone_assistant_marker() {
        assert_eq!(sanitize_assistant_text("[Assistant]: réponse"), "réponse");
    }
    #[test]
    fn preserves_normal_text() {
        assert_eq!(
            sanitize_assistant_text("réponse normale"),
            "réponse normale"
        );
    }
    #[test]
    fn preserves_tool_call_id() {
        let (_, calls) = parse_tool_calls(
            "```tool_call\n{\"id\":\"abc\",\"name\":\"file_read\",\"arguments\":{}}\n```",
        );
        assert_eq!(calls[0].id, "abc");
    }
    #[test]
    fn generates_stable_sequence_id() {
        let text = "```tool_call\n{\"name\":\"file_read\",\"arguments\":{}}\n```\n```tool_call\n{\"name\":\"search\",\"arguments\":{}}\n```";
        let (_, calls) = parse_tool_calls(text);
        assert_eq!(calls[0].id, "gemini_call_0");
        assert_eq!(calls[1].id, "gemini_call_1");
    }
    #[test]
    fn classifies_follow_up_as_action() {
        let (_, calls) = parse_tool_calls(r#"<FollowUp label="Run tests" query="cargo test" />"#);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].is_action());
    }
}
