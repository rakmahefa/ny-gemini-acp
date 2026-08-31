use serde_json::Value;

use super::types::ModelToolCall;

pub(super) fn parse_bare_json(text: &str, next_id: &mut usize) -> Option<ModelToolCall> {
    let value = serde_json::from_str::<Value>(text.trim()).ok()?;
    parse_tool_value(&value, next_id)
}

pub(super) fn parse_inline_tool_call(text: &str, next_id: &mut usize) -> Option<ModelToolCall> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("[tool_call ")?;
    let header_end = rest.find(']')?;
    let header = &rest[..header_end];
    let body = rest[header_end + 1..].trim();
    let (name, id) = match header.split_once(" id=") {
        Some((name, id)) => (name.trim(), Some(id.trim())),
        None => (header.trim(), None),
    };
    if name.is_empty() || body.is_empty() {
        return None;
    }
    let arguments = serde_json::from_str::<Value>(body).ok()?;
    let arguments = normalize_arguments(arguments)?;
    Some(ModelToolCall {
        id: id
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| allocate_call_id(next_id)),
        name: name.to_owned(),
        arguments,
    })
}

fn parse_tool_value(value: &Value, next_id: &mut usize) -> Option<ModelToolCall> {
    let name = value.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let arguments = value
        .get("arguments")
        .or_else(|| value.get("args"))?
        .clone();
    let arguments = normalize_arguments(arguments)?;
    let id = value
        .get("id")
        .or_else(|| value.get("call_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| allocate_call_id(next_id));
    Some(ModelToolCall {
        id,
        name: name.to_owned(),
        arguments,
    })
}

fn normalize_arguments(value: Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(value),
        Value::String(raw) => {
            let decoded = serde_json::from_str::<Value>(&raw).ok()?;
            decoded.is_object().then_some(decoded)
        }
        _ => None,
    }
}

pub(super) fn parse_follow_up_candidates(text: &str) -> Option<Vec<(String, String)>> {
    use agent_runtime::text::{find_tag_end, parse_follow_up_tag};

    let mut cursor = 0;
    let mut found = false;
    let mut calls = Vec::new();
    while let Some(relative_start) = text[cursor..].find(agent_runtime::text::FOLLOW_UP_TAG_PREFIX)
    {
        found = true;
        let start = cursor + relative_start;
        let after = start + agent_runtime::text::FOLLOW_UP_TAG_PREFIX.len();
        let end = find_tag_end(&text[after..])?;
        let absolute_end = after + end;
        let tag = &text[start..=absolute_end];
        calls.push(parse_follow_up_tag(tag)?);
        cursor = absolute_end + 1;
    }
    if found {
        Some(calls)
    } else {
        None
    }
}

pub(super) fn allocate_call_id(next_id: &mut usize) -> String {
    let id = format!("gemini_call_{}", *next_id);
    *next_id = (*next_id).saturating_add(1);
    id
}
