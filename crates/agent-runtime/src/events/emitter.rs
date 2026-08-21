use std::collections::{HashMap, VecDeque};

use super::integrity::{IntegrityError, TurnIntegrity, TurnPhase};
use super::{EventBus, EventContext, SemanticEvent, ToolEventContext};
use crate::ToolUiModel;

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

    pub fn sequence(&self) -> u64 { self.next_sequence }
    pub fn phase(&self) -> TurnPhase { self.integrity.phase() }
    pub fn is_terminal(&self) -> bool { self.integrity.phase() == TurnPhase::Terminal }

    fn context(&mut self) -> EventContext {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        EventContext::new(self.session_id.clone(), self.turn_id.clone(), sequence)
    }

    fn tool_context(&mut self, tool_call_id: impl Into<String>) -> ToolEventContext {
        ToolEventContext { event: self.context(), tool_call_id: tool_call_id.into() }
    }

    fn reject(&self, error: IntegrityError) -> bool {
        tracing::error!(session = %self.session_id, turn = %self.turn_id, error = %error.message, "rejected invalid semantic event transition");
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
        let semantic_id = if self.tool_bindings.contains_key(upstream_id) {
            self.allocate_tool_identity()
        } else {
            upstream_id.to_owned()
        };
        self.tool_bindings.entry(upstream_id.to_owned()).or_default().push_back(semantic_id.clone());
        semantic_id
    }

    fn resolve_tool_identity(&self, upstream_id: &str) -> Option<&str> {
        self.tool_bindings.get(upstream_id).and_then(VecDeque::front).map(String::as_str)
    }

    fn release_tool_identity(&mut self, upstream_id: &str) {
        let remove_binding = if let Some(queue) = self.tool_bindings.get_mut(upstream_id) {
            queue.pop_front();
            queue.is_empty()
        } else { false };
        if remove_binding { self.tool_bindings.remove(upstream_id); }
    }

    pub fn turn_started(&mut self) -> bool {
        if let Err(e) = self.integrity.turn_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::TurnStarted { context });
        true
    }

    pub fn assistant_started(&mut self) -> bool {
        if let Err(e) = self.integrity.assistant_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantStarted { context });
        true
    }

    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e) = self.integrity.assistant_delta() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantDelta { context, delta: delta.into() });
        true
    }

    pub fn assistant_completed(&mut self) -> bool {
        if let Err(e) = self.integrity.assistant_completed() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantCompleted { context });
        true
    }

    pub fn thinking_started(&mut self) -> bool {
        if let Err(e) = self.integrity.thinking_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingStarted { context });
        true
    }

    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e) = self.integrity.thinking_delta() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingDelta { context, delta: delta.into() });
        true
    }

    pub fn thinking_completed(&mut self) -> bool {
        if let Err(e) = self.integrity.thinking_completed() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingCompleted { context });
        true
    }

    pub fn tool_call_requested(&mut self, upstream_id: impl Into<String>, name: impl Into<String>) -> bool {
        self.tool_call_requested_with_ui(upstream_id, name, None)
    }

    pub fn tool_call_requested_with_ui(&mut self, upstream_id: impl Into<String>, name: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        let upstream_id = upstream_id.into();
        if upstream_id.is_empty() { return self.reject(IntegrityError::new("tool call identity must be non-empty")); }
        let semantic_id = self.bind_tool_identity(&upstream_id);
        if let Err(e) = self.integrity.tool_call_requested(&semantic_id) {
            self.release_tool_identity(&upstream_id);
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolCallRequested { context, name: name.into(), ui });
        true
    }

    pub fn permission_requested(&mut self, upstream_id: impl Into<String>) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("permission_requested references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.permission_requested(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::PermissionRequested { context });
        true
    }

    pub fn tool_execution_started(&mut self, upstream_id: impl Into<String>) -> bool {
        self.tool_execution_started_with_ui(upstream_id, None)
    }

    pub fn tool_execution_started_with_ui(&mut self, upstream_id: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("tool_execution_started references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.tool_execution_started(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolExecutionStarted { context, ui });
        true
    }

    pub fn tool_result_received(&mut self, upstream_id: impl Into<String>, result: impl Into<String>) -> bool {
        self.tool_result_received_with_ui(upstream_id, result, None)
    }

    pub fn tool_result_received_with_ui(&mut self, upstream_id: impl Into<String>, result: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("tool_result_received references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.tool_result_received(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolResultReceived { context, result: result.into(), ui });
        self.release_tool_identity(&upstream_id);
        true
    }

    fn finish_terminal(&mut self, event: &str) -> bool {
        if let Err(error) = self.integrity.finish_terminal_after_scopes(event) {
            return self.reject(error);
        }
        let context = self.context();
        match event {
            "turn_cancelled" => self.publish(SemanticEvent::TurnCancelled { context }),
            "turn_failed" => self.publish(SemanticEvent::TurnFailed { context }),
            "turn_completed" => self.publish(SemanticEvent::TurnCompleted { context }),
            _ => return self.reject(IntegrityError::new(format!("unknown terminal event {event}"))),
        }
        self.tool_bindings.clear();
        true
    }

    pub fn turn_cancelled(&mut self) -> bool { self.finish_terminal("turn_cancelled") }
    pub fn turn_failed(&mut self) -> bool { self.finish_terminal("turn_failed") }
    pub fn turn_completed(&mut self) -> bool { self.finish_terminal("turn_completed") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_event_is_rejected_while_a_tool_is_open() {
        let bus = EventBus::new();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(!e.turn_completed());
        assert!(!e.is_terminal());
        assert_eq!(e.sequence(), 2);
    }

    #[test]
    fn cancelled_turn_does_not_synthesize_tool_result() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(!e.turn_cancelled());
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SemanticEvent::TurnStarted { .. }));
        assert!(matches!(events[1], SemanticEvent::ToolCallRequested { .. }));
    }

    #[test]
    fn explicit_tool_result_allows_terminal_event() {
        let bus = EventBus::new();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(e.tool_execution_started("call-1"));
        assert!(e.tool_result_received("call-1", "ok"));
        assert!(e.turn_completed());
        assert!(e.is_terminal());
    }
}
