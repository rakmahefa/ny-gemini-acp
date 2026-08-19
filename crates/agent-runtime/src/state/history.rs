//! Canonical conversation history owned by the agent runtime.
//!
//! `History` owns a canonical semantic representation while temporarily exposing
//! a Vec-like legacy view. This lets the existing execution loop migrate without
//! flattening the persisted model back into provider-specific strings.

use std::ops::{Deref, DerefMut};

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Role;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryEntry {
    User { content: String },
    Assistant { content: String },
    ToolCall { id: String, name: String, arguments: Value },
    ToolResult { id: String, name: String, content: String, is_ok: bool },
}

impl HistoryEntry {
    pub fn role(&self) -> Role {
        match self {
            Self::User { .. } => Role::User,
            Self::Assistant { .. } | Self::ToolCall { .. } => Role::Assistant,
            Self::ToolResult { .. } => Role::Tool,
        }
    }

    pub fn approx_chars(&self) -> usize {
        match self {
            Self::User { content } | Self::Assistant { content } => content.chars().count(),
            Self::ToolCall { id, name, arguments } => {
                id.chars().count() + name.chars().count() + arguments.to_string().chars().count()
            }
            Self::ToolResult { id, name, content, .. } => {
                id.chars().count() + name.chars().count() + content.chars().count()
            }
        }
    }
}

/// Append operations accepted by `History::push`.
///
/// Structured entries become canonical immediately. Legacy `(Role, String)`
/// appends remain in the legacy buffer until normalization so flattened tool
/// markers can still be parsed into structured entries.
pub trait HistoryAppend {
    fn append_to(self, history: &mut History);
}

impl HistoryAppend for HistoryEntry {
    fn append_to(self, history: &mut History) {
        history.push_canonical(self);
    }
}

impl HistoryAppend for (Role, String) {
    fn append_to(self, history: &mut History) {
        history.legacy.push(self);
        history.dirty = true;
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    canonical: Vec<HistoryEntry>,
    legacy: Vec<(Role, String)>,
    dirty: bool,
}

impl PartialEq for History {
    fn eq(&self, other: &Self) -> bool { self.entries() == other.entries() }
}
impl Eq for History {}

impl From<Vec<(Role, String)>> for History {
    fn from(legacy: Vec<(Role, String)>) -> Self {
        Self { canonical: Self::normalize_legacy_entries(&legacy), legacy, dirty: false }
    }
}

impl History {
    pub fn new() -> Self { Self::default() }
    pub fn len(&self) -> usize { self.entries().len() }
    pub fn is_empty(&self) -> bool { self.entries().is_empty() }
    pub fn first(&self) -> Option<HistoryEntry> { self.entries().into_iter().next() }
    pub fn last(&self) -> Option<HistoryEntry> { self.entries().into_iter().last() }

    pub fn push<E>(&mut self, entry: E)
    where E: HistoryAppend {
        entry.append_to(self);
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.push_canonical(HistoryEntry::User { content: content.into() });
    }
    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.push_canonical(HistoryEntry::Assistant { content: content.into() });
    }
    pub fn push_tool_call(&mut self, id: impl Into<String>, name: impl Into<String>, arguments: Value) {
        self.push_canonical(HistoryEntry::ToolCall { id: id.into(), name: name.into(), arguments });
    }
    pub fn push_tool_result(&mut self, id: impl Into<String>, name: impl Into<String>, content: impl Into<String>, is_ok: bool) {
        self.push_canonical(HistoryEntry::ToolResult { id: id.into(), name: name.into(), content: content.into(), is_ok });
    }

    pub fn entries(&self) -> Vec<HistoryEntry> {
        if self.dirty { Self::normalize_legacy_entries(&self.legacy) } else { self.canonical.clone() }
    }

    pub fn replace(&mut self, entries: Vec<HistoryEntry>) {
        self.canonical = entries.clone();
        self.legacy = entries.into_iter().map(to_legacy).collect();
        self.dirty = false;
    }

    pub fn normalize_legacy(&mut self) {
        self.canonical = Self::normalize_legacy_entries(&self.legacy);
        self.dirty = false;
    }

    fn push_canonical(&mut self, entry: HistoryEntry) {
        self.legacy.push(to_legacy(entry.clone()));
        self.canonical.push(entry);
        self.dirty = false;
    }

    fn normalize_legacy_entries(legacy: &[(Role, String)]) -> Vec<HistoryEntry> {
        let mut normalized = Vec::with_capacity(legacy.len());
        let mut pending_tool_ids: Vec<(String, String)> = Vec::new();

        for (role, content) in legacy {
            match role {
                Role::User => normalized.push(HistoryEntry::User { content: content.clone() }),
                Role::Assistant => {
                    let mut plain = Vec::new();
                    for line in content.lines() {
                        if let Some((name, id, arguments)) = parse_tool_call_line(line) {
                            if !plain.is_empty() {
                                normalized.push(HistoryEntry::Assistant { content: plain.join("\n") });
                                plain.clear();
                            }
                            pending_tool_ids.push((name.clone(), id.clone()));
                            normalized.push(HistoryEntry::ToolCall { id, name, arguments });
                        } else {
                            plain.push(line.to_owned());
                        }
                    }
                    if !plain.is_empty() {
                        normalized.push(HistoryEntry::Assistant { content: plain.join("\n") });
                    }
                }
                Role::Tool => {
                    let (name, id, is_ok, clean_content) = parse_tool_result(content)
                        .unwrap_or_else(|| ("legacy".to_owned(), String::new(), true, content.clone()));
                    if id.is_empty() && name != "legacy" {
                        if let Some(position) = pending_tool_ids.iter().rposition(|(candidate, _)| candidate == &name) {
                            let resolved = pending_tool_ids.remove(position);
                            normalized.push(HistoryEntry::ToolResult { id: resolved.1, name, content: clean_content, is_ok });
                            continue;
                        }
                    }
                    normalized.push(HistoryEntry::ToolResult { id, name, content: clean_content, is_ok });
                }
            }
        }
        normalized
    }
}

impl Deref for History {
    type Target = Vec<(Role, String)>;
    fn deref(&self) -> &Self::Target { &self.legacy }
}

impl DerefMut for History {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        &mut self.legacy
    }
}

impl Serialize for History {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        self.entries().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for History {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let raw = Value::deserialize(deserializer)?;
        if let Ok(entries) = serde_json::from_value::<Vec<HistoryEntry>>(raw.clone()) {
            let legacy = entries.iter().cloned().map(to_legacy).collect();
            return Ok(Self { canonical: entries, legacy, dirty: false });
        }
        let legacy = serde_json::from_value::<Vec<(Role, String)>>(raw).map_err(serde::de::Error::custom)?;
        Ok(Self::from(legacy))
    }
}

fn to_legacy(entry: HistoryEntry) -> (Role, String) {
    match entry {
        HistoryEntry::User { content } => (Role::User, content),
        HistoryEntry::Assistant { content } => (Role::Assistant, content),
        HistoryEntry::ToolCall { id, name, arguments } => (Role::Assistant, format!("[tool_call {name} id={id}] {arguments}")),
        HistoryEntry::ToolResult { id, name, content, is_ok } => (Role::Tool, format!("[tool_result {name} id={id} status={}] {content}", if is_ok { "ok" } else { "error" })),
    }
}

fn parse_tool_call_line(line: &str) -> Option<(String, String, Value)> {
    let rest = line.strip_prefix("[tool_call ")?;
    let end = rest.find(']')?;
    let header = &rest[..end];
    let arguments = rest[end + 1..].trim();
    let mut parts = header.splitn(2, " id=");
    let name = parts.next()?.trim();
    let id = parts.next()?.trim();
    if name.is_empty() || id.is_empty() || arguments.is_empty() { return None; }
    Some((name.to_owned(), id.to_owned(), serde_json::from_str(arguments).ok()?))
}

fn parse_tool_result(content: &str) -> Option<(String, String, bool, String)> {
    let rest = content.strip_prefix("[tool_result ")?;
    let end = rest.find(']')?;
    let header = &rest[..end];
    let body = rest[end + 1..].trim().to_owned();
    let mut parts = header.split_whitespace();
    let name = parts.next()?.to_owned();
    let id = parts.find_map(|part| part.strip_prefix("id=").map(str::to_owned)).unwrap_or_default();
    let status = parts.find_map(|part| part.strip_prefix("status=").map(str::to_owned)).unwrap_or_else(|| "ok".to_owned());
    Some((name, id, status == "ok", body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_structured_tool_lifecycle() {
        let mut history = History::new();
        history.push_user("run tests");
        history.push_assistant("I will run them.");
        history.push_tool_call("call-1", "shell_exec", json!({"command": "cargo test"}));
        history.push_tool_result("call-1", "shell_exec", "ok", true);
        let raw = serde_json::to_string(&history).unwrap();
        let restored: History = serde_json::from_str(&raw).unwrap();
        assert_eq!(restored, history);
        assert!(matches!(restored.last(), Some(HistoryEntry::ToolResult { .. })));
    }

    #[test]
    fn loads_legacy_tuple_messages() {
        let legacy = r#"[["user","hello"],["assistant","world"],["tool","done"]]"#;
        let history: History = serde_json::from_str(legacy).unwrap();
        assert_eq!(history.len(), 3);
        assert!(matches!(history.first(), Some(HistoryEntry::User { content }) if content == "hello"));
        assert!(matches!(history.last(), Some(HistoryEntry::ToolResult { content, .. }) if content == "done"));
    }

    #[test]
    fn normalizes_flattened_tool_messages() {
        let mut history = History::new();
        history.push((Role::User, "run tests".into()));
        history.push((Role::Assistant, "I will run them.\n[tool_call shell_exec id=call-1] {\"command\":\"cargo test\"}".into()));
        history.push((Role::Tool, "[tool_result shell_exec status=ok] all green".into()));
        let entries = history.entries();
        assert!(matches!(entries.get(1), Some(HistoryEntry::Assistant { content }) if content == "I will run them."));
        assert!(matches!(entries.get(2), Some(HistoryEntry::ToolCall { id, name, .. }) if id == "call-1" && name == "shell_exec"));
        assert!(matches!(entries.last(), Some(HistoryEntry::ToolResult { id, name, content, is_ok }) if id == "call-1" && name == "shell_exec" && content.contains("all green") && *is_ok));
    }

    #[test]
    fn preserves_arbitrary_tool_result_content() {
        let mut history = History::new();
        history.push((Role::Assistant, "[tool_call file_read id=call-1] {\"path\":\"README.md\"}".into()));
        let payload = "[tool_result file_read status=ok] {\"quote\":\"[tool_call fake id=x]\",\"ellipsis\":\"…\"}";
        history.push((Role::Tool, payload.into()));
        let entries = history.entries();
        assert!(matches!(entries.last(), Some(HistoryEntry::ToolResult { id, name, content, is_ok }) if id == "call-1" && name == "file_read" && content.contains("[tool_call fake id=x]") && content.contains('…') && *is_ok));
    }
}
