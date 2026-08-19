//! Canonical conversation history owned by the agent runtime.
//!
//! The runtime deliberately stores semantic history instead of flattening tool
//! calls and tool results into display strings. Provider/ACP projections can
//! render this model differently without changing the persisted conversation.

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

    pub fn push(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
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

        // Compatibility with sessions written before the canonical history model:
        // `messages: [["user", "text"], ["assistant", "text"], ...]`.
        let legacy = serde_json::from_value::<Vec<(Role, String)>>(raw)
            .map_err(serde::de::Error::custom)?;
        let entries = legacy
            .into_iter()
            .map(|(role, content)| match role {
                Role::User => HistoryEntry::User { content },
                Role::Assistant => HistoryEntry::Assistant { content },
                Role::Tool => HistoryEntry::ToolResult {
                    id: String::new(),
                    name: "legacy".to_owned(),
                    content,
                    is_ok: true,
                },
            })
            .collect();
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
}
