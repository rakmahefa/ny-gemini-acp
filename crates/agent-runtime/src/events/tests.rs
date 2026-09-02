use super::*;
use crate::{ToolUiKind, ToolUiModel, ToolUiStatus};

#[test]
fn creates_turn_started_event() {
    let context = EventContext::new("session", "turn", 1);
    let event = SemanticEvent::TurnStarted { context };

    assert!(matches!(event, SemanticEvent::TurnStarted { .. }));
}

#[test]
fn keeps_tool_call_context_and_ui() {
    let event_context = EventContext::new("session", "turn", 2);
    let context = ToolEventContext {
        event: event_context,
        tool_call_id: "tool-a".into(),
    };
    let ui = ToolUiModel::generic("shell_exec", serde_json::json!({"command": "cargo test"}));

    let event = SemanticEvent::ToolExecutionStarted {
        context,
        ui: Some(ui.clone()),
    };

    match event {
        SemanticEvent::ToolExecutionStarted {
            context,
            ui: Some(actual),
        } => {
            assert_eq!(context.tool_call_id, "tool-a".into());
            assert_eq!(actual.kind, ToolUiKind::Generic);
            assert_eq!(actual.status, ToolUiStatus::Pending);
        }
        _ => unreachable!(),
    }
}

#[test]
fn preserves_event_order_sequence() {
    let first = EventContext::new("session", "turn", 1);
    let second = EventContext::new("session", "turn", 2);

    assert!(first.sequence < second.sequence);
    assert_eq!(first.session_id, second.session_id);
    assert_eq!(first.turn_id, second.turn_id);
}
