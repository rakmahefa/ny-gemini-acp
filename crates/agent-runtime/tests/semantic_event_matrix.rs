use agent_runtime::{EventBus, SemanticEvent, TurnEventEmitter, TurnPhase};

fn emitter() -> (EventBus, TurnEventEmitter) {
    let bus = EventBus::new();
    let _receiver = bus.subscribe_turn("turn");
    let emitter = TurnEventEmitter::new(bus.clone(), "session", "turn");
    (bus, emitter)
}

#[test]
fn valid_assistant_turn_is_accepted_and_terminal() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.assistant_started());
    assert!(emitter.assistant_delta("hello"));
    assert!(emitter.assistant_completed());
    assert!(emitter.turn_completed());
    assert!(emitter.is_terminal());
}

#[test]
fn valid_thinking_then_assistant_turn_is_accepted() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.assistant_started());
    assert!(emitter.thinking_started());
    assert!(emitter.thinking_delta("reasoning"));
    assert!(emitter.thinking_completed());
    assert!(emitter.assistant_delta("answer"));
    assert!(emitter.assistant_completed());
    assert!(emitter.turn_completed());
}

#[test]
fn valid_tool_permission_execution_result_sequence_is_accepted() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("tool-1", "shell"));
    assert!(emitter.permission_requested("tool-1"));
    assert!(emitter.tool_execution_started("tool-1"));
    assert!(emitter.tool_result_received("tool-1", "ok"));
    assert!(emitter.turn_completed());
}

#[test]
fn cancellation_is_terminal_and_closes_open_scopes() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("tool-1", "shell"));
    assert!(emitter.turn_cancelled());
    assert_eq!(emitter.phase(), TurnPhase::Terminal);
}

#[test]
fn failure_is_terminal() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.turn_failed());
    assert!(emitter.is_terminal());
}

#[test]
fn invalid_order_is_rejected_without_advancing_sequence() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    let sequence = emitter.sequence();
    assert!(!emitter.tool_result_received("unknown", "bad"));
    assert_eq!(emitter.sequence(), sequence);
    assert!(!emitter.is_terminal());
}

#[test]
fn second_terminal_event_is_rejected() {
    let (_bus, mut emitter) = emitter();
    assert!(emitter.turn_started());
    assert!(emitter.turn_failed());
    let sequence = emitter.sequence();
    assert!(!emitter.turn_completed());
    assert_eq!(emitter.sequence(), sequence);
}

#[test]
fn mandatory_transport_rejects_events_when_transport_is_absent() {
    let bus = EventBus::new();
    let mut emitter = TurnEventEmitter::new_with_required_transport(bus, "session", "turn");
    assert!(!emitter.turn_started());
    assert_eq!(emitter.phase(), TurnPhase::NotStarted);
}

#[test]
fn semantic_projection_preserves_the_canonical_event_sequence() {
    let (bus, mut emitter) = emitter();
    let mut receiver = bus.subscribe();
    assert!(emitter.turn_started());
    assert!(emitter.assistant_started());
    assert!(emitter.assistant_delta("hello"));
    assert!(emitter.assistant_completed());
    assert!(emitter.turn_completed());

    let mut sequences = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        sequences.push(match event {
            SemanticEvent::TurnStarted { context }
            | SemanticEvent::AssistantStarted { context }
            | SemanticEvent::AssistantDelta { context, .. }
            | SemanticEvent::AssistantCompleted { context }
            | SemanticEvent::ThinkingStarted { context }
            | SemanticEvent::ThinkingDelta { context, .. }
            | SemanticEvent::ThinkingCompleted { context }
            | SemanticEvent::TurnCancelled { context }
            | SemanticEvent::TurnFailed { context }
            | SemanticEvent::TurnCompleted { context } => context.sequence,
            SemanticEvent::ToolCallRequested { context, .. }
            | SemanticEvent::PermissionRequested { context }
            | SemanticEvent::ToolExecutionStarted { context, .. }
            | SemanticEvent::ToolResultReceived { context, .. } => context.event.sequence,
        });
    }
    assert_eq!(sequences, (0..5).collect::<Vec<_>>());
}
