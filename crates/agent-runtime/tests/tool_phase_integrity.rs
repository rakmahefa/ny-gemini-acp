use agent_runtime::{EventBus, SemanticEvent, TurnEventEmitter};

fn event_sequence(event: &SemanticEvent) -> u64 {
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
        | SemanticEvent::TurnCompleted { context } => context.sequence,
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => context.event.sequence,
    }
}

#[tokio::test]
async fn tool_result_cannot_bypass_execution() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    let mut emitter = TurnEventEmitter::new(bus, "session", "turn");

    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call", "shell_exec"));
    assert!(!emitter.tool_result_received("call", "forged result"));
    assert_eq!(emitter.sequence(), 2);

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events.iter().map(event_sequence).collect::<Vec<_>>(), vec![0, 1]);

    assert!(emitter.tool_execution_started("call"));
    assert!(emitter.tool_result_received("call", "real result"));
    assert!(emitter.turn_completed());
}

#[tokio::test]
async fn permission_result_remains_a_valid_terminal_path() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    let mut emitter = TurnEventEmitter::new(bus, "session", "turn");

    assert!(emitter.turn_started());
    assert!(emitter.tool_call_requested("call", "shell_exec"));
    assert!(emitter.permission_requested("call"));
    assert!(emitter.tool_result_received("call", "permission denied"));
    assert!(!emitter.tool_execution_started("call"));
    assert!(emitter.turn_completed());

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 5);
    assert!(matches!(&events[3], SemanticEvent::ToolResultReceived { .. }));
}

#[tokio::test]
async fn tool_call_cannot_overlap_an_open_assistant_stream() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    let mut emitter = TurnEventEmitter::new(bus, "session", "turn");

    assert!(emitter.turn_started());
    assert!(emitter.assistant_started());
    assert!(!emitter.tool_call_requested("call", "shell_exec"));
    assert_eq!(emitter.sequence(), 2);

    assert!(emitter.assistant_completed());
    assert!(emitter.tool_call_requested("call", "shell_exec"));
    assert!(emitter.tool_execution_started("call"));
    assert!(emitter.tool_result_received("call", "ok"));
    assert!(emitter.turn_completed());

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 7);
    assert_eq!(events.iter().map(event_sequence).collect::<Vec<_>>(), (0..7).collect::<Vec<_>>());
}
