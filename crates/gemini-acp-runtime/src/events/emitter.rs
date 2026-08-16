use super::{AcpSemanticEvent, EventBus, EventContext, ToolEventContext};

/// Owns the semantic sequence for one turn.
///
/// Runtime code should use this emitter instead of constructing event contexts
/// independently. This makes event ordering a property of the runtime turn,
/// rather than of individual producers.
#[derive(Clone)]
pub struct TurnEventEmitter {
    bus: EventBus,
    session_id: String,
    turn_id: String,
    next_sequence: u64,
}

impl TurnEventEmitter {
    pub fn new(bus: EventBus, session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            bus,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            next_sequence: 0,
        }
    }

    pub fn sequence(&self) -> u64 {
        self.next_sequence
    }

    fn context(&mut self) -> EventContext {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        EventContext::new(self.session_id.clone(), self.turn_id.clone(), sequence)
    }

    fn tool_context(&mut self, tool_call_id: impl Into<String>) -> ToolEventContext {
        ToolEventContext {
            event: self.context(),
            tool_call_id: tool_call_id.into(),
        }
    }

    fn publish(&self, event: AcpSemanticEvent) {
        if let Err(error) = self.bus.publish(event) {
            tracing::debug!(?error, "semantic event has no active subscribers");
        }
    }

    pub fn turn_started(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::TurnStarted { context });
    }

    pub fn assistant_started(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::AssistantStarted { context });
    }

    pub fn assistant_delta(&mut self, delta: impl Into<String>) {
        let context = self.context();
        self.publish(AcpSemanticEvent::AssistantDelta {
            context,
            delta: delta.into(),
        });
    }

    pub fn assistant_completed(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::AssistantCompleted { context });
    }

    pub fn thinking_started(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::ThinkingStarted { context });
    }

    pub fn thinking_delta(&mut self, delta: impl Into<String>) {
        let context = self.context();
        self.publish(AcpSemanticEvent::ThinkingDelta {
            context,
            delta: delta.into(),
        });
    }

    pub fn thinking_completed(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::ThinkingCompleted { context });
    }

    pub fn tool_call_requested(&mut self, tool_call_id: impl Into<String>, name: impl Into<String>) {
        let context = self.tool_context(tool_call_id);
        self.publish(AcpSemanticEvent::ToolCallRequested {
            context,
            name: name.into(),
        });
    }

    pub fn permission_requested(&mut self, tool_call_id: impl Into<String>) {
        let context = self.tool_context(tool_call_id);
        self.publish(AcpSemanticEvent::PermissionRequested { context });
    }

    pub fn tool_execution_started(&mut self, tool_call_id: impl Into<String>) {
        let context = self.tool_context(tool_call_id);
        self.publish(AcpSemanticEvent::ToolExecutionStarted { context });
    }

    pub fn tool_result_received(&mut self, tool_call_id: impl Into<String>, result: impl Into<String>) {
        let context = self.tool_context(tool_call_id);
        self.publish(AcpSemanticEvent::ToolResultReceived {
            context,
            result: result.into(),
        });
    }

    pub fn turn_cancelled(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::TurnCancelled { context });
    }

    pub fn turn_completed(&mut self) {
        let context = self.context();
        self.publish(AcpSemanticEvent::TurnCompleted { context });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allocates_monotonic_sequence_for_all_events() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut emitter = TurnEventEmitter::new(bus, "sess_1", "turn_1");

        emitter.turn_started();
        emitter.thinking_started();
        emitter.thinking_delta("hello");
        emitter.thinking_completed();
        emitter.assistant_started();
        emitter.assistant_delta("world");
        emitter.assistant_completed();
        emitter.turn_completed();

        let mut sequences = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            let sequence = match event {
                AcpSemanticEvent::TurnStarted { context }
                | AcpSemanticEvent::AssistantStarted { context }
                | AcpSemanticEvent::AssistantCompleted { context }
                | AcpSemanticEvent::ThinkingStarted { context }
                | AcpSemanticEvent::ThinkingCompleted { context }
                | AcpSemanticEvent::TurnCancelled { context }
                | AcpSemanticEvent::TurnCompleted { context } => context.sequence,
                AcpSemanticEvent::AssistantDelta { context, .. }
                | AcpSemanticEvent::ThinkingDelta { context, .. } => context.sequence,
                AcpSemanticEvent::ToolCallRequested { context, .. }
                | AcpSemanticEvent::PermissionRequested { context }
                | AcpSemanticEvent::ToolExecutionStarted { context }
                | AcpSemanticEvent::ToolResultReceived { context, .. } => context.event.sequence,
            };
            sequences.push(sequence);
        }

        assert_eq!(sequences, (0..8).collect::<Vec<_>>());
        assert_eq!(emitter.sequence(), 8);
    }

    #[tokio::test]
    async fn tool_events_share_turn_context_and_unique_sequences() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut emitter = TurnEventEmitter::new(bus, "sess_1", "turn_7");

        emitter.tool_call_requested("call_1", "shell_exec");
        emitter.tool_execution_started("call_1");
        emitter.tool_result_received("call_1", "completed");

        let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
        assert_eq!(events.len(), 3);

        for (expected, event) in events.into_iter().enumerate() {
            let context = match event {
                AcpSemanticEvent::ToolCallRequested { context, .. }
                | AcpSemanticEvent::PermissionRequested { context }
                | AcpSemanticEvent::ToolExecutionStarted { context }
                | AcpSemanticEvent::ToolResultReceived { context, .. } => context,
                _ => unreachable!(),
            };
            assert_eq!(context.event.session_id, "sess_1");
            assert_eq!(context.event.turn_id, "turn_7");
            assert_eq!(context.event.sequence, expected as u64);
            assert_eq!(context.tool_call_id, "call_1");
        }
    }
}
