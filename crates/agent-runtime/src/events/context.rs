use serde::{Deserialize, Serialize};

use crate::{SessionId, ToolCallId, TurnId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventContext {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub sequence: u64,
}

impl EventContext {
    pub fn new(
        session_id: impl Into<SessionId>,
        turn_id: impl Into<TurnId>,
        sequence: u64,
    ) -> Self {
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
    pub tool_call_id: ToolCallId,
}
