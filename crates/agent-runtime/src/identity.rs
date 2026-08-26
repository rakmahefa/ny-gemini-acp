use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

/// Strongly typed identity of a persisted agent session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for SessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Strongly typed identity of one execution turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(String);

impl TurnId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for TurnId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for TurnId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for TurnId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for TurnId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Strongly typed identity correlating a tool invocation across providers,
/// semantic events, UI projections and tool results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolCallId(String);

impl ToolCallId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for ToolCallId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ToolCallId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ToolCallId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for ToolCallId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionId, ToolCallId, TurnId};

    #[test]
    fn identities_preserve_wire_string_representation() {
        let session = SessionId::new("sess-1");
        let turn = TurnId::new("turn-1");
        let call = ToolCallId::new("call-1");

        assert_eq!(session.as_str(), "sess-1");
        assert_eq!(turn.as_str(), "turn-1");
        assert_eq!(call.as_str(), "call-1");
        assert_eq!(serde_json::to_string(&session).unwrap(), "\"sess-1\"");
        assert_eq!(serde_json::to_string(&turn).unwrap(), "\"turn-1\"");
        assert_eq!(serde_json::to_string(&call).unwrap(), "\"call-1\"");
    }

    #[test]
    fn identities_are_distinct_types() {
        fn needs_session(_: SessionId) {}
        fn needs_turn(_: TurnId) {}
        fn needs_call(_: ToolCallId) {}

        needs_session(SessionId::new("same"));
        needs_turn(TurnId::new("same"));
        needs_call(ToolCallId::new("same"));
    }
}
