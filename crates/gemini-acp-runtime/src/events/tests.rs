use super::*;

#[test]
fn creates_turn_started_event() {
    let context = EventContext::new("session", "turn", 1);
    let event = AcpSemanticEvent::TurnStarted { context };

    assert!(matches!(event, AcpSemanticEvent::TurnStarted { .. }));
}

#[test]
fn keeps_tool_call_context() {
    let event_context = EventContext::new("session", "turn", 2);
    let context = ToolEventContext {
        event: event_context,
        tool_call_id: "tool-a".into(),
    };

    let event = AcpSemanticEvent::ToolExecutionStarted { context };

    match event {
        AcpSemanticEvent::ToolExecutionStarted { context } => {
            assert_eq!(context.tool_call_id, "tool-a");
        }
        _ => unreachable!(),
    }
}
