use super::*;

#[test]
fn creates_turn_started_event() {
    let context = EventContext::new("session", "turn", 1);
    let event = SemanticEvent::TurnStarted { context };

    assert!(matches!(event, SemanticEvent::TurnStarted { .. }));
}

#[test]
fn keeps_tool_call_context() {
    let event_context = EventContext::new("session", "turn", 2);
    let context = ToolEventContext {
        event: event_context,
        tool_call_id: "tool-a".into(),
    };

    let event = SemanticEvent::ToolExecutionStarted { context };

    match event {
        SemanticEvent::ToolExecutionStarted { context } => {
            assert_eq!(context.tool_call_id, "tool-a");
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
