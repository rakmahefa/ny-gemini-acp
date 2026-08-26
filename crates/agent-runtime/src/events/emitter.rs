use std::collections::{HashMap, HashSet};

use super::integrity::{IntegrityError, ToolTerminalReason, TurnIntegrity, TurnPhase};
use super::{EventBus, EventContext, SemanticEvent, ToolEventContext};
use crate::{SessionId, ToolCallId, ToolUiModel, TurnId};

#[derive(Clone)]
pub struct TurnEventEmitter {
    bus: EventBus,
    session_id: SessionId,
    turn_id: TurnId,
    next_sequence: u64,
    integrity: TurnIntegrity,
    tool_bindings: HashMap<ToolCallId, ToolCallId>,
    seen_tool_ids: HashSet<ToolCallId>,
    require_transport: bool,
}

impl TurnEventEmitter {
    pub fn new(bus: EventBus, session_id: impl Into<SessionId>, turn_id: impl Into<TurnId>) -> Self {
        Self::build(bus, session_id, turn_id, false)
    }

    pub fn new_with_required_transport(
        bus: EventBus,
        session_id: impl Into<SessionId>,
        turn_id: impl Into<TurnId>,
    ) -> Self {
        Self::build(bus, session_id, turn_id, true)
    }

    fn build(
        bus: EventBus,
        session_id: impl Into<SessionId>,
        turn_id: impl Into<TurnId>,
        require_transport: bool,
    ) -> Self {
        Self {
            bus,
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            next_sequence: 0,
            integrity: TurnIntegrity::default(),
            tool_bindings: HashMap::new(),
            seen_tool_ids: HashSet::new(),
            require_transport,
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

    pub fn ensure_turn_started(&mut self) -> bool {
        match self.integrity.phase() {
            TurnPhase::NotStarted => self.turn_started(),
            TurnPhase::Active => true,
            TurnPhase::Terminal => false,
        }
    }

    fn transport_ready(&self) -> bool {
        if !self.require_transport || self.bus.has_turn_subscriber(self.turn_id.as_str()) {
            true
        } else {
            self.reject(IntegrityError::new(format!(
                "no ACP transport subscriber for turn {}",
                self.turn_id
            )));
            false
        }
    }

    fn context(&mut self) -> EventContext {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        EventContext::new(self.session_id.clone(), self.turn_id.clone(), sequence)
    }

    fn tool_context(&mut self, tool_call_id: ToolCallId) -> ToolEventContext {
        ToolEventContext {
            event: self.context(),
            tool_call_id,
        }
    }

    fn reject(&self, error: IntegrityError) -> bool {
        tracing::error!(session = %self.session_id, turn = %self.turn_id, error = %error.message, "rejected invalid semantic event transition");
        false
    }

    fn publish(&self, event: SemanticEvent) -> bool {
        if self.require_transport {
            self.bus.publish_global(event.clone());
            match self.bus.publish_turn(event) {
                Ok(()) => true,
                Err(error) => {
                    tracing::error!(session=%self.session_id, turn=%self.turn_id, error=%error, "mandatory ACP semantic transport failed");
                    false
                }
            }
        } else {
            self.bus.publish_global(event.clone());
            if self.bus.has_turn_subscriber(self.turn_id.as_str()) {
                if let Err(error) = self.bus.publish_turn(event) {
                    tracing::debug!(session=%self.session_id, turn=%self.turn_id, error=%error, "best-effort semantic turn transport failed");
                }
            }
            true
        }
    }

    fn bind_tool_identity(&mut self, upstream_id: ToolCallId) -> Result<ToolCallId, IntegrityError> {
        if !self.seen_tool_ids.insert(upstream_id.clone()) {
            return Err(IntegrityError::new(format!(
                "tool_call_id {upstream_id} was already used in this turn"
            )));
        }
        let semantic_id = upstream_id.clone();
        self.tool_bindings.insert(upstream_id, semantic_id.clone());
        Ok(semantic_id)
    }

    fn rollback_tool_binding(&mut self, upstream_id: &ToolCallId) {
        self.tool_bindings.remove(upstream_id);
        self.seen_tool_ids.remove(upstream_id);
    }

    fn resolve_tool_identity(&self, upstream_id: &ToolCallId) -> Option<&ToolCallId> {
        self.tool_bindings.get(upstream_id)
    }

    fn release_tool_identity(&mut self, upstream_id: &ToolCallId) {
        self.tool_bindings.remove(upstream_id);
    }

    pub fn turn_started(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.turn_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::TurnStarted { context })
    }

    pub fn assistant_started(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.assistant_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantStarted { context })
    }

    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.assistant_delta() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantDelta {
            context,
            delta: delta.into(),
        })
    }

    pub fn assistant_completed(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.assistant_completed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::AssistantCompleted { context })
    }

    /// Closes the assistant stream without emitting `AssistantCompleted`.
    ///
    /// This is a semantic handoff, not turn completion: the model has yielded to
    /// an action that still has to execute. The next observable semantic event is
    /// therefore the action/tool lifecycle itself.
    pub fn assistant_yields_to_action(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        match self.integrity.assistant_yields_to_action() {
            Ok(()) => true,
            Err(error) => self.reject(error),
        }
    }

    pub fn thinking_started(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.thinking_started() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingStarted { context })
    }

    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.thinking_delta() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingDelta {
            context,
            delta: delta.into(),
        })
    }

    pub fn thinking_completed(&mut self) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if let Err(e) = self.integrity.thinking_completed() {
            return self.reject(e);
        }
        let context = self.context();
        self.publish(SemanticEvent::ThinkingCompleted { context })
    }

    pub fn tool_call_requested(
        &mut self,
        upstream_id: impl Into<ToolCallId>,
        name: impl Into<String>,
    ) -> bool {
        self.tool_call_requested_with_ui(upstream_id, name, None)
    }

    pub fn tool_call_requested_with_ui(
        &mut self,
        upstream_id: impl Into<ToolCallId>,
        name: impl Into<String>,
        ui: Option<ToolUiModel>,
    ) -> bool {
        if !self.transport_ready() {
            return false;
        }
        let upstream_id = upstream_id.into();
        if upstream_id.is_empty() {
            return self.reject(IntegrityError::new("tool call identity must be non-empty"));
        }
        let semantic_id = match self.bind_tool_identity(upstream_id.clone()) {
            Ok(id) => id,
            Err(error) => return self.reject(error),
        };
        if let Err(e) = self.integrity.tool_call_requested(semantic_id.as_str()) {
            self.rollback_tool_binding(&upstream_id);
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        if self.publish(SemanticEvent::ToolCallRequested {
            context,
            name: name.into(),
            ui,
        }) {
            true
        } else {
            self.rollback_tool_binding(&upstream_id);
            false
        }
    }

    pub fn permission_requested(&mut self, upstream_id: impl Into<ToolCallId>) -> bool {
        if !self.transport_ready() {
            return false;
        }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).cloned() else {
            return self.reject(IntegrityError::new(format!(
                "permission_requested references unknown upstream tool {upstream_id}"
            )));
        };
        if let Err(e) = self.integrity.permission_requested(semantic_id.as_str()) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::PermissionRequested { context })
    }

    pub fn tool_execution_started(&mut self, upstream_id: impl Into<ToolCallId>) -> bool {
        self.tool_execution_started_with_ui(upstream_id, None)
    }

    pub fn tool_execution_started_with_ui(
        &mut self,
        upstream_id: impl Into<ToolCallId>,
        ui: Option<ToolUiModel>,
    ) -> bool {
        if !self.transport_ready() {
            return false;
        }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).cloned() else {
            return self.reject(IntegrityError::new(format!(
                "tool_execution_started references unknown upstream tool {upstream_id}"
            )));
        };
        if let Err(e) = self.integrity.tool_execution_started(semantic_id.as_str()) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        self.publish(SemanticEvent::ToolExecutionStarted { context, ui })
    }

    pub fn tool_result_received(
        &mut self,
        upstream_id: impl Into<ToolCallId>,
        result: impl Into<String>,
    ) -> bool {
        self.tool_result_received_with_ui(upstream_id, result, None)
    }

    pub fn tool_result_received_with_ui(
        &mut self,
        upstream_id: impl Into<ToolCallId>,
        result: impl Into<String>,
        ui: Option<ToolUiModel>,
    ) -> bool {
        if !self.transport_ready() {
            return false;
        }
        let upstream_id = upstream_id.into();
        let Some(semantic_id) = self.resolve_tool_identity(&upstream_id).cloned() else {
            return self.reject(IntegrityError::new(format!(
                "tool_result_received references unknown upstream tool {upstream_id}"
            )));
        };
        if let Err(e) = self.integrity.tool_result_received(semantic_id.as_str()) {
            return self.reject(e);
        }
        let context = self.tool_context(semantic_id);
        let emitted = self.publish(SemanticEvent::ToolResultReceived {
            context,
            result: result.into(),
            ui,
        });
        if emitted {
            self.release_tool_identity(&upstream_id);
        }
        emitted
    }

    fn abort_scopes(&mut self, reason: ToolTerminalReason) -> bool {
        if self.integrity.thinking_active() && !self.thinking_completed() {
            return false;
        }
        if self.integrity.assistant_active() && !self.assistant_completed() {
            return false;
        }
        self.integrity.abort_open_tools(reason).is_ok()
    }

    fn finish_terminal(&mut self, event: &str) -> bool {
        if !self.transport_ready() {
            return false;
        }
        if event == "turn_completed" {
            if let Err(error) = self.integrity.finish_terminal_after_scopes(event) {
                return self.reject(error);
            }
        } else {
            let reason = self.integrity.terminal_reason_for(event);
            if !self.abort_scopes(reason) {
                return self.reject(IntegrityError::new(format!(
                    "{event} could not abort open semantic scopes"
                )));
            }
            if let Err(error) = self.integrity.finish_terminal_after_scopes(event) {
                return self.reject(error);
            }
        }
        let context = self.context();
        let emitted = match event {
            "turn_cancelled" => self.publish(SemanticEvent::TurnCancelled { context }),
            "turn_failed" => self.publish(SemanticEvent::TurnFailed { context }),
            "turn_completed" => self.publish(SemanticEvent::TurnCompleted { context }),
            _ => {
                return self.reject(IntegrityError::new(format!(
                    "unknown terminal event {event}"
                )))
            }
        };
        if emitted {
            self.tool_bindings.clear();
            self.seen_tool_ids.clear();
        }
        emitted
    }

    pub fn turn_cancelled(&mut self) -> bool {
        self.finish_terminal("turn_cancelled")
    }
    pub fn turn_failed(&mut self) -> bool {
        self.finish_terminal("turn_failed")
    }
    pub fn turn_completed(&mut self) -> bool {
        self.finish_terminal("turn_completed")
    }

    #[cfg(test)]
    pub fn bind_count(&self) -> usize {
        self.tool_bindings.len()
    }
}
