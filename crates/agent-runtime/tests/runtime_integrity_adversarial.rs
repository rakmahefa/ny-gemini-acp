use agent_runtime::events::{EventBus, SemanticEvent, TurnEventEmitter};

fn emitter() -> (
    TurnEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<SemanticEvent>,
) {
    let bus = EventBus::new();
    let rx = bus.subscribe_turn("turn-adversarial");
    (
        TurnEventEmitter::new_with_required_transport(bus, "session", "turn-adversarial"),
        rx,
    )
}

#[test]
fn open_tool_blocks_successful_terminal_event() {
    let (mut emitter, _rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call-a", "shell"));
    assert!(!emitter.turn_completed());
    assert!(!emitter.is_terminal());
}

#[test]
fn cancellation_aborts_open_tool_without_fabricating_tool_result() {
    let (mut emitter, mut rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call-a", "shell"));
    assert!(emitter.turn_cancelled());
    assert!(emitter.is_terminal());
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], SemanticEvent::TurnStarted { .. }));
    assert!(matches!(events[1], SemanticEvent::ToolCallRequested { .. }));
    assert!(matches!(events[2], SemanticEvent::TurnCancelled { .. }));
    assert!(events
        .iter()
        .all(|event| !matches!(event, SemanticEvent::ToolResultReceived { .. })));
}

#[test]
fn failure_aborts_open_tool_without_fabricating_tool_result() {
    let (mut emitter, mut rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call-a", "shell"));
    assert!(emitter.turn_failed());
    assert!(emitter.is_terminal());
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[2], SemanticEvent::TurnFailed { .. }));
    assert!(events
        .iter()
        .all(|event| !matches!(event, SemanticEvent::ToolResultReceived { .. })));
}

#[test]
fn result_after_terminal_is_rejected() {
    let (mut emitter, _rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.assistant_started());
    assert!(emitter.assistant_delta("answer"));
    assert!(emitter.assistant_completed());
    assert!(emitter.turn_completed());
    assert!(!emitter.tool_result_received("unknown", "late"));
}

#[test]
fn tool_ids_are_unique_for_the_entire_turn() {
    let (mut emitter, _rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call-a", "shell"));
    assert!(emitter.tool_execution_started("call-a"));
    assert!(emitter.tool_result_received("call-a", "ok"));
    assert!(!emitter.tool_call_requested("call-a", "shell"));
}

#[test]
fn distinct_tool_ids_can_complete_out_of_order() {
    let (mut emitter, _rx) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call-a", "shell"));
    assert!(emitter.tool_call_requested("call-b", "search"));
    assert!(emitter.tool_execution_started("call-a"));
    assert!(emitter.tool_execution_started("call-b"));
    assert!(emitter.tool_result_received("call-b", "b"));
    assert!(emitter.tool_result_received("call-a", "a"));
    assert!(emitter.turn_completed());
}

#[test]
fn missing_transport_rejects_before_sequence_allocation() {
    let bus = EventBus::new();
    let mut emitter =
        TurnEventEmitter::new_with_required_transport(bus, "session", "turn-adversarial");
    assert!(!emitter.turn_started());
    assert_eq!(emitter.sequence(), 0);
    assert!(!emitter.is_terminal());
}

#[test]
fn global_broadcast_is_independent_from_mandatory_turn_transport() {
    let bus = EventBus::new();
    let mut global = bus.subscribe();
    let event = SemanticEvent::TurnStarted {
        context: agent_runtime::events::EventContext::new("session", "turn-adversarial", 0),
    };
    assert_eq!(bus.publish_global(event.clone()), 1);
    assert!(bus.publish_turn(event).is_err());
    assert!(global.try_recv().is_ok());
}
