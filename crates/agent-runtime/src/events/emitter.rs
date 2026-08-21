use std::collections::{HashMap, HashSet};

use super::integrity::{IntegrityError, TurnIntegrity, TurnPhase};
use super::{EventBus, EventContext, SemanticEvent, ToolEventContext};
use crate::ToolUiModel;

#[derive(Clone)]
pub struct TurnEventEmitter {
    bus: EventBus,
    session_id: String,
    turn_id: String,
    next_sequence: u64,
    integrity: TurnIntegrity,
    tool_bindings: HashMap<String, String>,
    seen_tool_ids: HashSet<String>,
}

impl TurnEventEmitter {
    pub fn new(bus: EventBus, session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            bus,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            next_sequence: 0,
            integrity: TurnIntegrity::default(),
            tool_bindings: HashMap::new(),
            seen_tool_ids: HashSet::new(),
        }
    }

    pub fn sequence(&self) -> u64 { self.next_sequence }
    pub fn phase(&self) -> TurnPhase { self.integrity.phase() }
    pub fn is_terminal(&self) -> bool { self.integrity.phase() == TurnPhase::Terminal }

    fn transport_ready(&self) -> bool {
        if self.bus.has_turn_subscriber(&self.turn_id) { true }
        else {
            self.reject(IntegrityError::new(format!("no ACP transport subscriber for turn {}", self.turn_id)));
            false
        }
    }

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

    fn publish(&self, event: SemanticEvent) -> bool {
        self.bus.publish_global(event.clone());
        match self.bus.publish_turn(event) {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(session=%self.session_id, turn=%self.turn_id, error=%error, "mandatory ACP semantic transport failed");
                false
            }
        }
    }

    fn bind_tool_identity(&mut self, upstream_id: &str) -> Result<String, IntegrityError> {
        if !self.seen_tool_ids.insert(upstream_id.to_owned()) {
            return Err(IntegrityError::new(format!(
                "tool_call_id {upstream_id} was already used in this turn"
            )));
        }
        let semantic_id = upstream_id.to_owned();
        self.tool_bindings.insert(upstream_id.to_owned(), semantic_id.clone());
        Ok(semantic_id)
    }

    fn rollback_tool_binding(&mut self, upstream_id: &str) {
        self.tool_bindings.remove(upstream_id);
        self.seen_tool_ids.remove(upstream_id);
    }

    fn resolve_tool_identity(&self, upstream_id: &str) -> Option<&str> {
        self.tool_bindings.get(upstream_id).map(String::as_str)
    }

    fn release_tool_identity(&mut self, upstream_id: &str) {
        self.tool_bindings.remove(upstream_id);
    }

    pub fn turn_started(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.turn_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::TurnStarted { context })
    }

    pub fn assistant_started(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.assistant_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantStarted { context })
    }

    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.assistant_delta() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantDelta { context, delta: delta.into() })
    }

    pub fn assistant_completed(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.assistant_completed() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::AssistantCompleted { context })
    }

    pub fn thinking_started(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.thinking_started() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingStarted { context })
    }

    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.thinking_delta() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingDelta { context, delta: delta.into() })
    }

    pub fn thinking_completed(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(e) = self.integrity.thinking_completed() { return self.reject(e); }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingCompleted { context })
    }

    pub fn tool_call_requested(&mut self, upstream_id: impl Into<String>, name: impl Into<String>) -> bool {
        self.tool_call_requested_with_ui(upstream_id, name, None)
    }

    pub fn tool_call_requested_with_ui(&mut self, upstream_id: impl Into<String>, name: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready() { return false; }
        let upstream_id = upstream_id.into();
        if upstream_id.is_empty() { return self.reject(IntegrityError::new("tool call identity must be non-empty")); }
        let semantic_id = match self.bind_tool_identity(&upstream_id) {
            Ok(id) => id,
            Err(error) => return self.reject(error),
        };
        if let Err(e) = self.integrity.tool_call_requested(&semantic_id) {
            self.rollback_tool_binding(&upstream_id);
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        if self.publish(SemanticEvent::ToolCallRequested { context, name: name.into(), ui }) { true }
        else { self.rollback_tool_binding(&upstream_id); false }
    }

    pub fn permission_requested(&mut self, upstream_id: impl Into<String>) -> bool {
        if !self.transport_ready() { return false; }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("permission_requested references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.permission_requested(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::PermissionRequested { context })
    }

    pub fn tool_execution_started(&mut self, upstream_id: impl Into<String>) -> bool {
        self.tool_execution_started_with_ui(upstream_id, None)
    }

    pub fn tool_execution_started_with_ui(&mut self, upstream_id: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready() { return false; }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("tool_execution_started references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.tool_execution_started(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolExecutionStarted { context, ui })
    }

    pub fn tool_result_received(&mut self, upstream_id: impl Into<String>, result: impl Into<String>) -> bool {
        self.tool_result_received_with_ui(upstream_id, result, None)
    }

    pub fn tool_result_received_with_ui(&mut self, upstream_id: impl Into<String>, result: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready() { return false; }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).map(str::to_owned) else {
            return self.reject(IntegrityError::new(format!("tool_result_received references unknown upstream tool {upstream_id}")));
        };
        if let Err(e) = self.integrity.tool_result_received(&semantic_id) { return self.reject(e); }
        let context = self.tool_context(semantic_id);
        let emitted = self.publish(SemanticEvent::ToolResultReceived { context, result: result.into(), ui });
        if emitted { self.release_tool_identity(&upstream_id); }
        emitted
    }

    fn finish_terminal(&mut self, event: &str) -> bool {
        if !self.transport_ready() { return false; }
        if let Err(error) = self.integrity.finish_terminal_after_scopes(event) { return self.reject(error); }
        let context = self.context();
        let emitted = match event {
            "turn_cancelled" => self.publish(SemanticEvent::TurnCancelled { context }),
            "turn_failed" => self.publish(SemanticEvent::TurnFailed { context }),
            "turn_completed" => self.publish(SemanticEvent::TurnCompleted { context }),
            _ => return self.reject(IntegrityError::new(format!("unknown terminal event {event}"))),
        };
        if emitted {
            self.tool_bindings.clear();
            self.seen_tool_ids.clear();
        }
        emitted
    }

    pub fn turn_cancelled(&mut self) -> bool { self.finish_terminal("turn_cancelled") }
    pub fn turn_failed(&mut self) -> bool { self.finish_terminal("turn_failed") }
    pub fn turn_completed(&mut self) -> bool { self.finish_terminal("turn_completed") }

    #[cfg(test)]
    pub fn bind_count(&self) -> usize { self.tool_bindings.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter() -> (TurnEventEmitter, tokio::sync::mpsc::UnboundedReceiver<SemanticEvent>) {
        let bus = EventBus::new();
        let rx = bus.subscribe_turn("t");
        (TurnEventEmitter::new(bus, "s", "t"), rx)
    }

    #[test]
    fn terminal_event_is_rejected_while_a_tool_is_open() {
        let (mut e, _rx) = emitter();
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(!e.turn_completed());
        assert!(!e.is_terminal());
        assert_eq!(e.sequence(), 2);
    }

    #[test]
    fn cancelled_turn_does_not_synthesize_tool_result() {
        let (mut e, mut rx) = emitter();
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
        let (mut e, _rx) = emitter();
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(e.tool_execution_started("call-1"));
        assert!(e.tool_result_received("call-1", "ok"));
        assert!(e.turn_completed());
        assert!(e.is_terminal());
    }

    #[test]
    fn duplicate_upstream_tool_ids_are_rejected_even_after_completion() {
        let (mut e, _rx) = emitter();
        assert!(e.turn_started());
        assert!(e.tool_call_requested("call-1", "shell"));
        assert!(e.tool_execution_started("call-1"));
        assert!(e.tool_result_received("call-1", "ok"));
        assert!(!e.tool_call_requested("call-1", "shell"));
        assert_eq!(e.bind_count(), 0);
    }

    #[test]
    fn a_missing_mandatory_transport_rejects_before_state_mutation() {
        let bus = EventBus::new();
        let mut e = TurnEventEmitter::new(bus, "s", "t");
        assert!(!e.turn_started());
        assert_eq!(e.phase(), TurnPhase::NotStarted);
        assert_eq!(e.sequence(), 0);
    }
}
