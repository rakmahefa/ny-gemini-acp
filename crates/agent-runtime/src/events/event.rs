use serde::{Deserialize, Serialize};

use super::{EventContext, ToolEventContext};
use crate::ToolUiModel;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SemanticEvent {
    TurnStarted {
        context: EventContext,
    },
    AssistantStarted {
        context: EventContext,
    },
    AssistantDelta {
        context: EventContext,
        delta: String,
    },
    AssistantCompleted {
        context: EventContext,
    },
    ThinkingStarted {
        context: EventContext,
    },
    ThinkingDelta {
        context: EventContext,
        delta: String,
    },
    ThinkingCompleted {
        context: EventContext,
    },
    ToolCallRequested {
        context: ToolEventContext,
        name: String,
        ui: Option<ToolUiModel>,
    },
    PermissionRequested {
        context: ToolEventContext,
    },
    ToolExecutionStarted {
        context: ToolEventContext,
        ui: Option<ToolUiModel>,
    },
    ToolResultReceived {
        context: ToolEventContext,
        result: String,
        ui: Option<ToolUiModel>,
    },
    TurnCancelled {
        context: EventContext,
    },
    TurnFailed {
        context: EventContext,
    },
    TurnCompleted {
        context: EventContext,
    },
}
