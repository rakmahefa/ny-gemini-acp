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

impl SemanticEvent {
    /// P-12 : extracteurs de contexte uniques — remplaçent les 3 copies
    /// (bus.rs, replay.rs supprimé, tests) d'anciennes fonctions libres.
    pub fn context(&self) -> &EventContext {
        match self {
            SemanticEvent::TurnStarted { context }
            | SemanticEvent::AssistantStarted { context }
            | SemanticEvent::AssistantDelta { context, .. }
            | SemanticEvent::AssistantCompleted { context }
            | SemanticEvent::ThinkingStarted { context }
            | SemanticEvent::ThinkingDelta { context, .. }
            | SemanticEvent::ThinkingCompleted { context }
            | SemanticEvent::TurnCancelled { context }
            | SemanticEvent::TurnFailed { context }
            | SemanticEvent::TurnCompleted { context } => context,
            SemanticEvent::ToolCallRequested { context, .. }
            | SemanticEvent::PermissionRequested { context }
            | SemanticEvent::ToolExecutionStarted { context, .. }
            | SemanticEvent::ToolResultReceived { context, .. } => &context.event,
        }
    }

    pub fn turn_id(&self) -> &str {
        self.context().turn_id.as_str()
    }

    pub fn session_id(&self) -> &str {
        self.context().session_id.as_str()
    }

    pub fn sequence(&self) -> u64 {
        self.context().sequence
    }

    pub fn kind(&self) -> &'static str {
        match self {
            SemanticEvent::TurnStarted { .. } => "turn_started",
            SemanticEvent::AssistantStarted { .. } => "assistant_started",
            SemanticEvent::AssistantDelta { .. } => "assistant_delta",
            SemanticEvent::AssistantCompleted { .. } => "assistant_completed",
            SemanticEvent::ThinkingStarted { .. } => "thinking_started",
            SemanticEvent::ThinkingDelta { .. } => "thinking_delta",
            SemanticEvent::ThinkingCompleted { .. } => "thinking_completed",
            SemanticEvent::ToolCallRequested { .. } => "tool_call_requested",
            SemanticEvent::PermissionRequested { .. } => "permission_requested",
            SemanticEvent::ToolExecutionStarted { .. } => "tool_execution_started",
            SemanticEvent::ToolResultReceived { .. } => "tool_result_received",
            SemanticEvent::TurnCancelled { .. } => "turn_cancelled",
            SemanticEvent::TurnFailed { .. } => "turn_failed",
            SemanticEvent::TurnCompleted { .. } => "turn_completed",
        }
    }

    pub fn tool_call_id(&self) -> Option<&str> {
        match self {
            SemanticEvent::ToolCallRequested { context, .. }
            | SemanticEvent::PermissionRequested { context }
            | SemanticEvent::ToolExecutionStarted { context, .. }
            | SemanticEvent::ToolResultReceived { context, .. } => {
                Some(context.tool_call_id.as_str())
            }
            _ => None,
        }
    }
}
