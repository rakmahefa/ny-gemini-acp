use std::collections::{HashMap, VecDeque};

use super::integrity::{IntegrityError, TurnIntegrity, TurnPhase};
use super::{SemanticEvent, EventBus, EventContext, ToolEventContext};

/// Owns and validates the semantic sequence for one turn.
/// Invalid transitions never reach the event bus and do not consume a sequence number.
#[derive(Clone)]
pub struct TurnEventEmitter {
    bus: EventBus,
    session_id: String,
    turn_id: String,
    next_sequence: u64,
    next_tool_invocation: u64,
    integrity: TurnIntegrity,
    /// Maps upstream tool-call identities to semantic identities scoped to this turn.
    ///
    /// Gemini may legally restart a stream-local call counter on every internal round.
    /// Semantic events must not reuse a terminalized identity. A queue also keeps
    /// the mapping deterministic if an upstream producer ever emits duplicate IDs before
    /// their lifecycle is fully terminalized.
    tool_bindings: HashMap<String, VecDeque<String>>,
}

impl TurnEventEmitter {
    pub fn new(bus: EventBus, session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            bus,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            next_sequence: 0,
            next_tool_invocation: 0,
            integrity: TurnIntegrity::default(),
            tool_bindings: HashMap::new(),
        }
    }

    pub fn sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn phase(&self) -> TurnPhase {
        self.integrity.phase()
    }

    pub fn is_terminal(&self) -> bool {
        self.integrity.phase() == TurnPhase::Terminal
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

    fn reject(&self, error: IntegrityError) -> bool {
        tracing::error!(
            session = %self.session_id,
            turn = %self.turn_id,
            error = %error.message,
            "rejected invalid semantic event transition"
        );
        false
    }

    fn publish(&self, event: SemanticEvent) {
        if let Err(error) = self.bus.publish(event) {
            tracing::debug!(?error, "semantic event has no active subscribers");
        }
    }

    fn allocate_tool_identity(&mut self) -> String {
        let index = self.next_tool_invocation;
        self.next_tool_invocation = self.next_tool_invocation.saturating_add(1);
        format!("{}/tool_{}", self.turn_id, index)
    }

    fn bind_tool_identity(&mut self, upstream_id: &str) -> String {
        let semantic_id = self.allocate_tool_identity();
        self.tool_bindings
            .entry(upstream_id.to_owned())
            .or_default()
            .push_back(semantic_id.clone());
        semantic_id
    }

    fn resolve_tool_identity(&self, upstream_id: &str) -> Option<&str> {
        self.tool_bindings
            .get(upstream_id)
            .and_then(VecDeque::front)
            .map(String::as_str)
    }

    fn release_tool_identity(&mut self, upstream_id: &str) {
        let remove_binding = if let Some(queue) = self.tool_bindings.get_mut(upstream_id) {
            queue.pop_front();
            queue.is_empty()
        } else {
            false
        };

        if remove_binding {
            self.tool_bindings.remove(upstream_id);
        }
    }

    pub fn turn_started(&mut self) -> bool {
        if let Err(e) = self.integrity.turn_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::TurnStarted { context });
        true
    }

    pub fn assistant_started(&mut self) -> bool {
        if let Err(e) = self.integrity.assistant_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantStarted { context });
        true
    }

    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e) = self.integrity.assistant_delta() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantDelta {
            context,
            delta: delta.into(),
        });
        true
    }

    pub fn assistant_completed(&mut self) -> bool {
        if let Err(e) = self.integrity.assistant_completed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantCompleted { context });
        true
    }

    pub fn thinking_started(&mut self) -> bool {
        if let Err(e) = self.integrity.thinking_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingStarted { context });
        true
    }

    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e) = self.integrity.thinking_delta() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingDelta {
            context,
            delta: delta.into(),
        });
        true
    }

    pub fn thinking_completed(&mut self) -> bool {
        if let Err(e) = self.integrity.thinking_completed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingCompleted { context });
        true
    }

    /// Records a tool invocation using a semantic identity independent from the
    /// upstream Gemini call ID. All later lifecycle events resolve through this binding.
    pub fn tool_call_requested(
        &mut self,
        upstream_id: impl Into<String>,
        name: impl Into<String>,
    ) -> bool {
        let upstream_id = upstream_id.into();
        if upstream_id.is_empty() {
            return self.reject(IntegrityError::new("tool call identity must be non-empty"));
        }

        let semantic_id = self.bind_tool_identity(&upstream_id);
        if let Err(e) = self.integrity.tool_call_requested(&semantic_id) {
            self.release_tool_identity(&upstream_id);
            return self.reject(e);
        }

        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolCallRequested {
            context,
            name: name.into(),
        });
        true
    }

    pub fn permission_requested(&mut self, upstream_id: impl Into<String>) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!(
                "permission_requested references unknown upstream tool {upstream_id}"
            )));
        };

        if let Err(e) = self.integrity.permission_requested(&semantic_id) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::PermissionRequested { context });
        true
    }

    pub fn tool_execution_started(&mut self, upstream_id: impl Into<String>) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!(
                "tool_execution_started references unknown upstream tool {upstream_id}"
            )));
        };

        if let Err(e) = self.integrity.tool_execution_started(&semantic_id) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolExecutionStarted { context });
        true
    }

    pub fn tool_result_received(
        &mut self,
        upstream_id: impl Into<String>,
        result: impl Into<String>,
    ) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!(
                "tool_result_received references unknown upstream tool {upstream_id}"
            )));
        };

        if let Err(e) = self.integrity.tool_result_received(&semantic_id) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolResultReceived {
            context,
            result: result.into(),
        });
        self.release_tool_identity(&upstream_id);
        true
    }

    pub fn turn_cancelled(&mut self) -> bool {
        if let Err(e) = self.integrity.turn_cancelled() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::TurnCancelled { context });
        self.tool_bindings.clear();
        true
    }

    pub fn turn_failed(&mut self) -> bool {
        if let Err(e) = self.integrity.turn_failed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::TurnFailed { context });
        self.tool_bindings.clear();
        true
    }

    pub fn turn_completed(&mut self) -> bool {
        if let Err(e) = self.integrity.turn_completed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::TurnCompleted { context });
        self.tool_bindings.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(event: &SemanticEvent) -> u64 {
        match event {
            SemanticEvent::TurnStarted { context }
            | SemanticEvent::AssistantStarted { context }
            | SemanticEvent::AssistantCompleted { context }
            | SemanticEvent::ThinkingStarted { context }
            | SemanticEvent::ThinkingCompleted { context }
            | SemanticEvent::TurnCancelled { context }
            | SemanticEvent::TurnFailed { context }
            | SemanticEvent::TurnCompleted { context } => context.sequence,
            SemanticEvent::AssistantDelta { context, .. }
            | SemanticEvent::ThinkingDelta { context, .. } => context.sequence,
            SemanticEvent::ToolCallRequested { context, .. }
            | SemanticEvent::PermissionRequested { context }
            | SemanticEvent::ToolExecutionStarted { context }
            | SemanticEvent::ToolResultReceived { context, .. } => context.event.sequence,
        }
    }

    fn tool_id(event: &SemanticEvent) -> &str {
        match event {
            SemanticEvent::ToolCallRequested { context, .. }
            | SemanticEvent::PermissionRequested { context }
            | SemanticEvent::ToolExecutionStarted { context }
            | SemanticEvent::ToolResultReceived { context, .. } => &context.tool_call_id,
            _ => panic!("expected tool event"),
        }
    }

    #[tokio::test]
    async fn accepted_events_have_contiguous_sequences() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.assistant_started());
        assert!(e.thinking_started());
        assert!(e.thinking_delta("x"));
        assert!(e.thinking_completed());
        assert!(e.assistant_delta("y"));
        assert!(e.assistant_completed());
        assert!(e.tool_call_requested("c", "shell_exec"));
        assert!(e.permission_requested("c"));
        assert!(e.tool_execution_started("c"));
        assert!(e.tool_result_received("c", "ok"));
        assert!(e.turn_completed());
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            events.iter().map(seq).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        assert_eq!(e.sequence(), 12);
        assert!(e.is_terminal());
    }

    #[tokio::test]
    async fn invalid_events_are_rejected_without_sequence_consumption() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(!e.turn_completed());
        assert_eq!(e.sequence(), 0);
        assert!(rx.try_recv().is_err());
        assert!(!e.is_terminal());
        assert!(e.turn_started());
        assert!(!e.turn_started());
        assert_eq!(e.sequence(), 1);
        assert_eq!(seq(&rx.try_recv().unwrap()), 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_is_terminal_even_with_open_lifecycle() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.assistant_started());
        assert!(e.thinking_started());
        assert!(e.tool_call_requested("c", "shell_exec"));
        assert!(e.turn_cancelled());
        assert!(!e.turn_completed());
        assert!(!e.assistant_delta("late"));
        assert!(e.is_terminal());

        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(matches!(
            events.last(),
            Some(SemanticEvent::TurnCancelled { .. })
        ));
        assert_eq!(events.len(), 5);
    }

    #[tokio::test]
    async fn failure_is_terminal_and_sequence_stays_contiguous() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.assistant_started());
        assert!(e.turn_failed());
        assert!(!e.turn_completed());
        assert!(e.is_terminal());
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(events.iter().map(seq).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(matches!(
            events.last(),
            Some(SemanticEvent::TurnFailed { .. })
        ));
        assert_eq!(e.sequence(), 3);
    }

    #[tokio::test]
    async fn repeated_upstream_tool_ids_get_distinct_semantic_identities() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "session", "turn_xyz");

        assert!(e.turn_started());
        assert!(e.tool_call_requested("gemini_call_0", "shell_exec"));
        assert!(e.permission_requested("gemini_call_0"));
        assert!(e.tool_execution_started("gemini_call_0"));
        assert!(e.tool_result_received("gemini_call_0", "first"));

        // Gemini restarts its stream-local counter in a later round.
        assert!(e.tool_call_requested("gemini_call_0", "shell_exec"));
        assert!(e.permission_requested("gemini_call_0"));
        assert!(e.tool_execution_started("gemini_call_0"));
        assert!(e.tool_result_received("gemini_call_0", "second"));
        assert!(e.turn_completed());

        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let tool_events: Vec<_> = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    SemanticEvent::ToolCallRequested { .. }
                        | SemanticEvent::PermissionRequested { .. }
                        | SemanticEvent::ToolExecutionStarted { .. }
                        | SemanticEvent::ToolResultReceived { .. }
                )
            })
            .collect();

        assert_eq!(tool_events.len(), 8);

        let first_id = tool_id(tool_events[0]);
        let second_id = tool_id(tool_events[4]);
        assert_eq!(first_id, "turn_xyz/tool_0");
        assert_eq!(second_id, "turn_xyz/tool_1");
        assert_ne!(first_id, second_id);

        for event in tool_events.iter().take(4) {
            assert_eq!(tool_id(event), first_id);
        }
        for event in tool_events.iter().skip(4) {
            assert_eq!(tool_id(event), second_id);
        }
    }

    #[tokio::test]
    async fn semantic_identity_is_scoped_to_the_turn_and_not_the_session() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        let mut first = TurnEventEmitter::new(bus.clone(), "session", "turn_a");
        assert!(first.turn_started());
        assert!(first.tool_call_requested("gemini_call_0", "shell_exec"));
        assert!(first.permission_requested("gemini_call_0"));
        assert!(first.tool_execution_started("gemini_call_0"));
        assert!(first.tool_result_received("gemini_call_0", "ok"));
        assert!(first.turn_completed());

        let mut second = TurnEventEmitter::new(bus, "session", "turn_b");
        assert!(second.turn_started());
        assert!(second.tool_call_requested("gemini_call_0", "shell_exec"));
        assert!(second.permission_requested("gemini_call_0"));
        assert!(second.tool_execution_started("gemini_call_0"));
        assert!(second.tool_result_received("gemini_call_0", "ok"));
        assert!(second.turn_completed());

        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let ids: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                SemanticEvent::ToolCallRequested { context, .. }
                | SemanticEvent::PermissionRequested { context }
                | SemanticEvent::ToolExecutionStarted { context }
                | SemanticEvent::ToolResultReceived { context, .. } => {
                    Some(context.tool_call_id.as_str())
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            ids,
            vec![
                "turn_a/tool_0",
                "turn_a/tool_0",
                "turn_a/tool_0",
                "turn_a/tool_0",
                "turn_b/tool_0",
                "turn_b/tool_0",
                "turn_b/tool_0",
                "turn_b/tool_0",
            ]
        );
        assert_ne!(ids[0], ids[4]);
    }
}
