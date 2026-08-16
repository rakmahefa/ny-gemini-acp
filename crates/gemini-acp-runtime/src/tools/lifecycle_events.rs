use crate::events::{AcpSemanticEvent, EventContext, EventBus, ToolEventContext};

use super::lifecycle::ToolLifecycleState;

pub fn emit_tool_state(
    bus: &EventBus,
    context: ToolEventContext,
    state: ToolLifecycleState,
    tool_name: &str,
) {
    let event = match state {
        ToolLifecycleState::Pending => AcpSemanticEvent::ToolCallRequested {
            context,
            name: tool_name.to_owned(),
        },
        ToolLifecycleState::Permission => {
            AcpSemanticEvent::PermissionRequested { context }
        }
        ToolLifecycleState::Executing => {
            AcpSemanticEvent::ToolExecutionStarted { context }
        }
        ToolLifecycleState::Completed | ToolLifecycleState::Failed => {
            AcpSemanticEvent::ToolResultReceived {
                context,
                result: state.to_string(),
            }
        }
        ToolLifecycleState::Cancelled => AcpSemanticEvent::ToolResultReceived {
            context,
            result: "cancelled".into(),
        },
    };

    let _ = bus.publish(event);
}

pub fn context(session_id: impl Into<String>, turn_id: impl Into<String>, sequence: u64, tool_call_id: impl Into<String>) -> ToolEventContext {
    ToolEventContext {
        event: EventContext::new(session_id, turn_id, sequence),
        tool_call_id: tool_call_id.into(),
    }
}
