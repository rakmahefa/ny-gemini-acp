use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::events::{EventContext, SemanticEvent};
use agent_runtime::Cancellation;
use tokio::sync::broadcast;

use super::notify::{
    notify_reasoning, notify_text, notify_tool_call, notify_tool_call_update,
};

#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("semantic event transport lagged and skipped {skipped} event(s)")]
    Lagged { skipped: u64 },
    #[error("semantic event sequence gap: expected {expected}, received {actual}")]
    SequenceGap { expected: u64, actual: u64 },
    #[error("semantic event transport closed before the turn reached a terminal event")]
    Closed,
    #[error("ACP notification failed: {0}")]
    Acp(#[from] AcpError),
}

#[derive(Debug, Default)]
struct SequenceTracker {
    next: Option<u64>,
}

impl SequenceTracker {
    fn observe(&mut self, context: &EventContext) -> Result<(), ProjectionError> {
        let expected = self.next.unwrap_or(0);
        if context.sequence != expected {
            return Err(ProjectionError::SequenceGap {
                expected,
                actual: context.sequence,
            });
        }
        self.next = Some(expected.saturating_add(1));
        Ok(())
    }
}

fn event_context(event: &SemanticEvent) -> &EventContext {
    match event {
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

/// Projects validated semantic runtime events into ACP.
///
/// This transport is deliberately fail-closed: a lagged broadcast receiver,
/// sequence gap, or closed bus is a transport integrity failure. We never skip
/// events and continue, because doing so would make ACP observe an incomplete
/// semantic turn while the runtime believes the turn is intact.
pub async fn project(
    mut events: broadcast::Receiver<SemanticEvent>,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    turn_id: &str,
    cancellation: Cancellation,
) -> Result<(), ProjectionError> {
    let mut sequence = SequenceTracker::default();

    loop {
        match events.recv().await {
            Ok(event) => {
                let context = event_context(&event);
                if context.turn_id != turn_id {
                    continue;
                }
                sequence.observe(context)?;

                match event {
                    SemanticEvent::AssistantDelta { delta, .. } => {
                        notify_text(cx, session_id, message_id, delta)?;
                    }
                    SemanticEvent::ThinkingDelta { delta, .. } => {
                        notify_reasoning(cx, session_id, message_id, delta)?;
                    }
                    SemanticEvent::ToolCallRequested {
                        context,
                        ui: Some(ui),
                        ..
                    } => notify_tool_call(cx, session_id, &context.tool_call_id, &ui)?,
                    SemanticEvent::ToolExecutionStarted {
                        context,
                        ui: Some(ui),
                    } => notify_tool_call_update(cx, session_id, &context.tool_call_id, &ui)?,
                    SemanticEvent::ToolResultReceived {
                        context,
                        ui: Some(ui),
                        ..
                    } => notify_tool_call_update(cx, session_id, &context.tool_call_id, &ui)?,
                    SemanticEvent::TurnCancelled { .. }
                    | SemanticEvent::TurnFailed { .. }
                    | SemanticEvent::TurnCompleted { .. } => break,
                    _ => {}
                }
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                cancellation.cancel();
                return Err(ProjectionError::Lagged { skipped });
            }
            Err(broadcast::error::RecvError::Closed) => {
                cancellation.cancel();
                return Err(ProjectionError::Closed);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(sequence: u64) -> EventContext {
        EventContext::new("session", "turn", sequence)
    }

    #[test]
    fn sequence_tracker_accepts_contiguous_events() {
        let mut tracker = SequenceTracker::default();
        assert!(tracker.observe(&context(0)).is_ok());
        assert!(tracker.observe(&context(1)).is_ok());
        assert!(tracker.observe(&context(2)).is_ok());
    }

    #[test]
    fn sequence_tracker_rejects_missing_events() {
        let mut tracker = SequenceTracker::default();
        tracker.observe(&context(0)).unwrap();
        let error = tracker.observe(&context(2)).unwrap_err();
        assert!(matches!(
            error,
            ProjectionError::SequenceGap {
                expected: 1,
                actual: 2
            }
        ));
    }

    #[test]
    fn sequence_tracker_requires_the_turn_to_start_at_zero() {
        let mut tracker = SequenceTracker::default();
        let error = tracker.observe(&context(1)).unwrap_err();
        assert!(matches!(
            error,
            ProjectionError::SequenceGap {
                expected: 0,
                actual: 1
            }
        ));
    }
}
