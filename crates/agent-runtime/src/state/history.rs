//! Canonical conversation history owned by the agent runtime.
//!
//! The runtime stores semantic history instead of making persisted tool
//! lifecycles depend on display strings. The `push((Role, String))` adapter
//! remains temporarily supported so older execution paths can migrate without
//! breaking persisted sessions.

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Role;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryEntry {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_ok: bool,
    },
}

impl From<(Role, String)> for HistoryEntry {
    fn from((role, content): (Role, String)) -> Self {
        match role {
            Role::User => Self::User { content },
            Role::Assistant => Self::Assistant { content },
            Role::Tool => Self::ToolResult {
                id: String::new(),
                name: "legacy".to_owned(),
                content,
                is_ok: true,
            },
        }
    }
}

impl HistoryEntry {
    pub fn role(&self) -> Role {
        match self {
            Self::User { .. } => Role::User,
            Self::Assistant { .. } | Self::ToolCall { .. } => Role::Assistant,
            Self::ToolResult { .. } => Role::Tool,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::User { content } | Self::Assistant { content } | Self::ToolResult { content, .. } => content,
            Self::ToolCall { .. } => "",
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct History {
    entries: Vec<HistoryEntry>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn first(&self) -> Option<&HistoryEntry> {
        self.entries.first()
    }

    pub fn last(&self) -> Option<&HistoryEntry> {
        self.entries.last()
    }

    pub fn push<E>(&mut self, entry: E)
    where
        E: Into<HistoryEntry>,
    {
        self.entries.push(entry.into());
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.push(HistoryEntry::User { content: content.into() });
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.push(HistoryEntry::Assistant { content: content.into() });
    }

    pub fn push_tool_call(&mut self, id: impl Into<String>, name: impl Into<String>, arguments: Value) {
        self.push(HistoryEntry::ToolCall { id: id.into(), name: name.into(), arguments });
    }

    pub fn push_tool_result(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
        is_ok: bool,
    ) {
        self.push(HistoryEntry::ToolResult {
            id: id.into(),
            name: name.into(),
            content: content.into(),
            is_ok,
        });
    }

    pub fn iter(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    pub fn replace(&mut self, entries: Vec<HistoryEntry>) {
        self.entries = entries;
    }

    pub fn to_vec(&self) -> Vec<HistoryEntry> {
        self.entries.clone()
    }

    pub fn formatted_chars(&self) -> usize {
        self.entries.iter().map(HistoryEntry::approx_chars).sum()
    }

    /// Converts legacy flattened tool messages produced by the pre-history-model
    /// execution path into canonical tool-call/result entries.
    pub fn normalize_legacy(&mut self) {
        let mut normalized = Vec::with_capacity(self.entries.len());
        let mut unresolved: Vec<(String, String)> = Vec::new();

        for entry in self.entries.drain(..) {
            match entry {
                HistoryEntry::Assistant { content } => {
                    let mut plain = Vec::new();
                    for line in content.lines() {
                        if let Some((name, id, arguments)) = parse_tool_call_line(line) {
                            if !plain.is_empty() {
                                normalized.push(HistoryEntry::Assistant {
                                    content: plain.join("\n"),
                                });
                                plain.clear();
                            }
                            unresolved.push((name.clone(), id.clone()));
                            normalized.push(HistoryEntry::ToolCall { id, name, arguments });
                        } else {
                            plain.push(line.to_owned());
                        }
                    }
                    if !plain.is_empty() {
                        normalized.push(HistoryEntry::Assistant {
                            content: plain.join("\n"),
                        });
                    }
                }
                HistoryEntry::ToolResult {
                    id,
                    name,
                    content,
                    is_ok,
                } if id.is_empty() && name == "legacy" => {
                    let (resolved_name, resolved_id) = unresolved
                        .iter()
                        .rev()
                        .next()
                        .cloned()
                        .unwrap_or_else(|| (name.clone(), id.clone()));
                    unresolved.pop();
                    normalized.push(HistoryEntry::ToolResult {
                        id: resolved_id,
                        name: resolved_name,
                        content,
                        is_ok,
                    });
                }
                other => normalized.push(other),
            }
        }

        self.entries = normalized;
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
    if name.is_empty() || id.is_empty() || arguments.is_empty() {
        return None;
    }
    let value = serde_json::from_str(arguments).ok()?;
    Some((name.to_owned(), id.to_owned(), value))
}

impl<'de> Deserialize<'de> for History {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        if let Ok(entries) = serde_json::from_value::<Vec<HistoryEntry>>(raw.clone()) {
            return Ok(Self { entries });
        }

        let legacy = serde_json::from_value::<Vec<(Role, String)>>(raw)
            .map_err(serde::de::Error::custom)?;
        let entries = legacy.into_iter().map(HistoryEntry::from).collect();
        Ok(Self { entries })
    }
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
        history.push((
            Role::Assistant,
            "I will run them.\n[tool_call shell_exec id=call-1] {\"command\":\"cargo test\"}".into(),
        ));
        history.push((Role::Tool, "[tool_result shell_exec status=ok] all green".into()));
        history.normalize_legacy();

        assert!(matches!(history.iter().nth(1), Some(HistoryEntry::Assistant { content }) if content == "I will run them."));
        assert!(matches!(history.iter().nth(2), Some(HistoryEntry::ToolCall { id, name, .. }) if id == "call-1" && name == "shell_exec"));
        assert!(matches!(history.last(), Some(HistoryEntry::ToolResult { id, name, content, is_ok }) if id == "call-1" && name == "shell_exec" && content.contains("all green") && *is_ok));
    }
}
