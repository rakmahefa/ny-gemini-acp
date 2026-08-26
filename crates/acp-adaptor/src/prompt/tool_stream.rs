use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::events::{EventContext, SemanticEvent};
use agent_runtime::Cancellation;
use tokio::sync::mpsc;

use super::notify::{notify_reasoning, notify_text, notify_tool_call, notify_tool_call_update};

#[derive(Debug, Default)]
pub struct ProjectionMetrics {
    pub sequence_gaps: AtomicU64,
    pub unexpected_turns: AtomicU64,
    pub acp_failures: AtomicU64,
    pub transport_closes: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionMetricsSnapshot {
    pub sequence_gaps: u64,
    pub unexpected_turns: u64,
    pub acp_failures: u64,
    pub transport_closes: u64,
}

impl ProjectionMetrics {
    pub fn snapshot(&self) -> ProjectionMetricsSnapshot {
        ProjectionMetricsSnapshot {
            sequence_gaps: self.sequence_gaps.load(Ordering::Relaxed),
            unexpected_turns: self.unexpected_turns.load(Ordering::Relaxed),
            acp_failures: self.acp_failures.load(Ordering::Relaxed),
            transport_closes: self.transport_closes.load(Ordering::Relaxed),
        }
    }
}

static METRICS: OnceLock<ProjectionMetrics> = OnceLock::new();

pub fn projection_metrics() -> &'static ProjectionMetrics {
    METRICS.get_or_init(ProjectionMetrics::default)
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("semantic event transport channel closed before the turn reached a terminal event")]
    Closed,
    #[error("semantic event sequence gap: expected {expected}, received {actual}")]
    SequenceGap { expected: u64, actual: u64 },
    #[error("semantic event belongs to unexpected turn {expected}, received {actual}")]
    UnexpectedTurn { expected: String, actual: String },
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

#[derive(Debug)]
enum ProjectionAction {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        ui: agent_runtime::ToolUiModel,
    },
    ToolUpdate {
        id: String,
        ui: agent_runtime::ToolUiModel,
    },
    Terminal,
    Ignore,
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

fn project_event(
    sequence: &mut SequenceTracker,
    event: SemanticEvent,
    turn_id: &str,
) -> Result<ProjectionAction, ProjectionError> {
    let context = event_context(&event);
    if context.turn_id != turn_id {
        return Err(ProjectionError::UnexpectedTurn {
            expected: turn_id.to_owned(),
            actual: context.turn_id.clone(),
        });
    }
    sequence.observe(context)?;

    Ok(match event {
        SemanticEvent::AssistantDelta { delta, .. } => ProjectionAction::Text(delta),
        SemanticEvent::ThinkingDelta { delta, .. } => ProjectionAction::Reasoning(delta),
        SemanticEvent::ToolCallRequested {
            context,
            ui: Some(ui),
            ..
        } => ProjectionAction::ToolCall {
            id: context.tool_call_id,
            ui,
        },
        SemanticEvent::ToolExecutionStarted {
            context,
            ui: Some(ui),
        }
        | SemanticEvent::ToolResultReceived {
            context,
            ui: Some(ui),
            ..
        } => ProjectionAction::ToolUpdate {
            id: context.tool_call_id,
            ui,
        },
        SemanticEvent::TurnCancelled { .. }
        | SemanticEvent::TurnFailed { .. }
        | SemanticEvent::TurnCompleted { .. } => ProjectionAction::Terminal,
        _ => ProjectionAction::Ignore,
    })
}

/// Projects validated semantic runtime events into ACP.
///
/// The dedicated per-turn queue is lossless. Sequence validation remains in place as
/// a second integrity barrier, so a producer bug that reorders, duplicates, or mutates
/// event sequencing is still detected rather than rendered as an apparently valid ACP turn.
pub async fn project(
    mut events: mpsc::UnboundedReceiver<SemanticEvent>,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    turn_id: &str,
    cancellation: Cancellation,
) -> Result<(), ProjectionError> {
    let metrics = projection_metrics();
    let mut sequence = SequenceTracker::default();

    loop {
        let event = match events.recv().await {
            Some(event) => event,
            None => {
                metrics.transport_closes.fetch_add(1, Ordering::Relaxed);
                cancellation.cancel();
                return Err(ProjectionError::Closed);
            }
        };

        let action = match project_event(&mut sequence, event, turn_id) {
            Ok(action) => action,
            Err(error @ ProjectionError::UnexpectedTurn { .. }) => {
                metrics.unexpected_turns.fetch_add(1, Ordering::Relaxed);
                cancellation.cancel();
                return Err(error);
            }
            Err(error @ ProjectionError::SequenceGap { .. }) => {
                metrics.sequence_gaps.fetch_add(1, Ordering::Relaxed);
                cancellation.cancel();
                return Err(error);
            }
            Err(error) => {
                cancellation.cancel();
                return Err(error);
            }
        };

        let notification = match action {
            ProjectionAction::Text(text) => notify_text(cx, session_id, message_id, text),
            ProjectionAction::Reasoning(text) => notify_reasoning(cx, session_id, message_id, text),
            ProjectionAction::ToolCall { id, ui } => notify_tool_call(cx, session_id, &id, &ui),
            ProjectionAction::ToolUpdate { id, ui } => {
                notify_tool_call_update(cx, session_id, &id, &ui)
            }
            ProjectionAction::Terminal => return Ok(()),
            ProjectionAction::Ignore => Ok(()),
        };

        if let Err(error) = notification {
            metrics.acp_failures.fetch_add(1, Ordering::Relaxed);
            cancellation.cancel();
            return Err(ProjectionError::Acp(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::events::TurnEventEmitter;
    use serde_json::json;

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

    #[test]
    fn metrics_are_snapshotable() {
        let metrics = ProjectionMetrics::default();
        metrics.sequence_gaps.fetch_add(2, Ordering::Relaxed);
        metrics.acp_failures.fetch_add(3, Ordering::Relaxed);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.sequence_gaps, 2);
        assert_eq!(snapshot.acp_failures, 3);
    }

    #[tokio::test]
    async fn semantic_event_to_acp_projection_rejects_injected_loss_before_notification() {
        let bus = agent_runtime::EventBus::new();
        let mut receiver = bus.subscribe_turn("turn-e2e");
        let mut emitter = TurnEventEmitter::new(bus, "session-e2e", "turn-e2e");
        assert!(emitter.turn_started());
        assert!(emitter.assistant_started());
        assert!(emitter.assistant_delta("hello"));
        assert!(emitter.assistant_completed());
        assert!(emitter.turn_completed());

        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        assert_eq!(events.len(), 5);

        events.retain(|event| event_context(event).sequence != 2);

        let mut sequence = SequenceTracker::default();
        let mut actions = Vec::new();
        for event in events {
            match project_event(&mut sequence, event, "turn-e2e") {
                Ok(action) => {
                    let terminal = matches!(action, ProjectionAction::Terminal);
                    actions.push(action);
                    if terminal {
                        break;
                    }
                }
                Err(error) => {
                    assert!(matches!(
                        error,
                        ProjectionError::SequenceGap {
                            expected: 2,
                            actual: 3
                        }
                    ));
                    assert!(actions
                        .iter()
                        .all(|action| !matches!(action, ProjectionAction::Terminal)));
                    return;
                }
            }
        }
        panic!("injected event loss was not rejected");
    }

    #[tokio::test]
    async fn semantic_event_to_acp_projection_preserves_tool_id_and_ui() {
        let bus = agent_runtime::EventBus::new();
        let mut receiver = bus.subscribe_turn("turn-tool-e2e");
        let mut emitter = TurnEventEmitter::new(bus, "session-e2e", "turn-tool-e2e");
        let ui = agent_runtime::ToolUiModel::generic("shell_exec", json!({"command":"pwd"}));
        assert!(emitter.turn_started());
        assert!(emitter.tool_call_requested_with_ui(
            "provider-call-7",
            "shell_exec",
            Some(ui.clone())
        ));
        assert!(
            emitter.tool_execution_started_with_ui("provider-call-7", Some(ui.clone().running()))
        );
        assert!(emitter.tool_result_received_with_ui(
            "provider-call-7",
            "ok",
            Some(ui.clone().completed(true, Some(json!({"text":"ok"}))))
        ));
        assert!(emitter.turn_completed());

        let mut sequence = SequenceTracker::default();
        let mut tool_call_id = None;
        let mut tool_status = None;
        while let Ok(event) = receiver.try_recv() {
            match project_event(&mut sequence, event, "turn-tool-e2e").unwrap() {
                ProjectionAction::ToolCall { id, ui } => {
                    tool_call_id = Some(id);
                    tool_status = Some(ui.status);
                }
                ProjectionAction::ToolUpdate { id, ui } => {
                    assert_eq!(id, "provider-call-7");
                    tool_status = Some(ui.status);
                }
                _ => {}
            }
        }
        assert_eq!(tool_call_id.as_deref(), Some("provider-call-7"));
        assert_eq!(tool_status, Some(agent_runtime::ToolUiStatus::Succeeded));
    }
}
