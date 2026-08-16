use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContext {
    pub session_id: String,
    pub turn_id: String,
    pub sequence: u64,
}

impl EventContext {
    pub fn new(session_id: impl Into<String>, turn_id: impl Into<String>, sequence: u64) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            sequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEventContext {
    pub event: EventContext,
    pub tool_call_id: String,
}
