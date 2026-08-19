use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::SemanticEvent;
use tokio::sync::broadcast;

use super::notify::{
    notify_reasoning, notify_text, notify_tool_call, notify_tool_call_update,
};

/// Projects validated semantic runtime events into ACP, including the native
/// tool-call lifecycle carried by `ToolUiModel`.
pub async fn project(
    mut events: broadcast::Receiver<SemanticEvent>,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    turn_id: &str,
) -> Result<(), AcpError> {
    loop {
        match events.recv().await {
            Ok(event) => {
                let event_turn = match &event {
                    SemanticEvent::TurnStarted { context }
                    | SemanticEvent::AssistantStarted { context }
                    | SemanticEvent::AssistantDelta { context, .. }
                    | SemanticEvent::AssistantCompleted { context }
                    | SemanticEvent::ThinkingStarted { context }
                    | SemanticEvent::ThinkingDelta { context, .. }
                    | SemanticEvent::ThinkingCompleted { context }
                    | SemanticEvent::TurnCancelled { context }
                    | SemanticEvent::TurnFailed { context }
                    | SemanticEvent::TurnCompleted { context } => &context.turn_id,
                    SemanticEvent::ToolCallRequested { context, .. }
                    | SemanticEvent::PermissionRequested { context }
                    | SemanticEvent::ToolExecutionStarted { context, .. }
                    | SemanticEvent::ToolResultReceived { context, .. } => &context.event.turn_id,
                };
                if event_turn != turn_id {
                    continue;
                }

                match event {
                    SemanticEvent::AssistantDelta { delta, .. } => {
                        notify_text(cx, session_id, message_id, delta)?;
                    }
                    SemanticEvent::ThinkingDelta { delta, .. } => {
                        notify_reasoning(cx, session_id, message_id, delta)?;
                    }
                    SemanticEvent::ToolCallRequested { context, ui, .. } => {
                        if let Some(ui) = ui {
                            notify_tool_call(cx, session_id, &context.tool_call_id, &ui)?;
                        }
                    }
                    SemanticEvent::ToolExecutionStarted { context, ui } => {
                        if let Some(ui) = ui {
                            notify_tool_call_update(cx, session_id, &context.tool_call_id, &ui)?;
                        }
                    }
                    SemanticEvent::ToolResultReceived { context, ui, .. } => {
                        if let Some(ui) = ui {
                            notify_tool_call_update(cx, session_id, &context.tool_call_id, &ui)?;
                        }
                    }
                    SemanticEvent::TurnCancelled { .. }
                    | SemanticEvent::TurnFailed { .. }
                    | SemanticEvent::TurnCompleted { .. } => break,
                    _ => {}
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
