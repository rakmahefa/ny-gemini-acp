use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::types::{ModelToolCall, FOLLOW_UP_PREFIX};

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
    let arguments = value.get("arguments").or_else(|| value.get("args"))?.clone();
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

/// Tool arguments are a JSON object in the semantic contract. A few Gemini
/// dialects serialize that object as a JSON string; accept that representation
/// only when it decodes back into an object, never an arbitrary scalar.
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
    let mut cursor = 0;
    let mut found = false;
    let mut calls = Vec::new();
    while let Some(relative_start) = text[cursor..].find(FOLLOW_UP_PREFIX) {
        found = true;
        let start = cursor + relative_start;
        let after = start + FOLLOW_UP_PREFIX.len();
        let end = find_tag_end(&text[after..])?;
        let absolute_end = after + end;
        let tag = &text[start..=absolute_end];
        calls.push(parse_follow_up_tag(tag)?);
        cursor = absolute_end + 1;
    }
    if found { Some(calls) } else { None }
}

fn parse_follow_up_tag(tag: &str) -> Option<(String, String)> {
    let inner = tag.strip_prefix(FOLLOW_UP_PREFIX)?.strip_suffix('>')?.trim();
    let inner = inner.strip_suffix('/').unwrap_or(inner).trim();
    let attrs = parse_attributes(inner);
    let label = attrs.get("label")?.trim();
    let query = attrs.get("query")?.trim();
    if label.is_empty() || query.is_empty() {
        return None;
    }
    Some((decode_xml(label), decode_xml(query)))
}

fn parse_attributes(input: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; }
        if index >= bytes.len() || bytes[index] == b'/' { break; }
        let key_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' { index += 1; }
        if key_start == index { index += 1; continue; }
        let key = &input[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; }
        if index >= bytes.len() || bytes[index] != b'=' { break; }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; }
        if index >= bytes.len() { break; }
        let value = if bytes[index] == b'\'' || bytes[index] == b'"' {
            let quote = bytes[index];
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote { index += 1; }
            let value = input[value_start..index].to_owned();
            if index < bytes.len() { index += 1; }
            value
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() { index += 1; }
            input[value_start..index].to_owned()
        };
        attrs.insert(key.to_ascii_lowercase(), value);
    }
    attrs
}

fn decode_xml(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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

pub(super) fn allocate_call_id(next_id: &mut usize) -> String {
    let id = format!("gemini_call_{}", *next_id);
    *next_id = (*next_id).saturating_add(1);
    id
}
