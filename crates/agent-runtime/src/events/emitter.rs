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
    pub fn new(bus: EventBus, session_id: impl Into<SessionId>, turn_id: impl Into<TurnId>) -> Self { Self::build(bus, session_id, turn_id, false) }
    pub fn new_with_required_transport(bus: EventBus, session_id: impl Into<SessionId>, turn_id: impl Into<TurnId>) -> Self { Self::build(bus, session_id, turn_id, true) }
    fn build(bus: EventBus, session_id: impl Into<SessionId>, turn_id: impl Into<TurnId>, require_transport: bool) -> Self {
        Self { bus, session_id: session_id.into(), turn_id: turn_id.into(), next_sequence: 0, integrity: TurnIntegrity::default(), tool_bindings: HashMap::new(), seen_tool_ids: HashSet::new(), require_transport }
    }
    pub fn sequence(&self) -> u64 { self.next_sequence }
    pub fn phase(&self) -> TurnPhase { self.integrity.phase() }
    pub fn is_terminal(&self) -> bool { self.integrity.phase() == TurnPhase::Terminal }
    pub fn ensure_turn_started(&mut self) -> bool { match self.integrity.phase() { TurnPhase::NotStarted => self.turn_started(), TurnPhase::Active => true, TurnPhase::Terminal => false } }
    fn transport_ready(&self) -> bool {
        if !self.require_transport || self.bus.has_turn_subscriber(self.turn_id.as_str()) { true } else { self.reject(IntegrityError::new(format!("no ACP transport subscriber for turn {}", self.turn_id))); false }
    }
    fn proposed_context(&self) -> EventContext { EventContext::new(self.session_id.clone(), self.turn_id.clone(), self.next_sequence) }
    fn reject(&self, error: IntegrityError) -> bool { tracing::error!(session=%self.session_id, turn=%self.turn_id, error=%error.message, "rejected invalid semantic event transition"); false }
    fn publish(&self, event: SemanticEvent) -> bool {
        if self.require_transport {
            match self.bus.publish_turn(event.clone()) {
                Ok(()) => { self.bus.publish_global(event); true }
                Err(error) => { tracing::error!(session=%self.session_id, turn=%self.turn_id, error=%error, "mandatory ACP semantic transport failed"); false }
            }
        } else {
            self.bus.publish_global(event.clone());
            if self.bus.has_turn_subscriber(self.turn_id.as_str()) { let _ = self.bus.publish_turn(event); }
            true
        }
    }
    fn bind_tool_identity(&mut self, upstream_id: ToolCallId) -> Result<ToolCallId, IntegrityError> {
        if !self.seen_tool_ids.insert(upstream_id.clone()) { return Err(IntegrityError::new(format!("tool_call_id {upstream_id} was already used in this turn"))); }
        let semantic_id = upstream_id.clone(); self.tool_bindings.insert(upstream_id, semantic_id.clone()); Ok(semantic_id)
    }
    fn rollback_tool_binding(&mut self, upstream_id: &ToolCallId) { self.tool_bindings.remove(upstream_id); self.seen_tool_ids.remove(upstream_id); }
    fn resolve_tool_identity(&self, upstream_id: &ToolCallId) -> Option<&ToolCallId> { self.tool_bindings.get(upstream_id) }
    fn release_tool_identity(&mut self, upstream_id: &ToolCallId) { self.tool_bindings.remove(upstream_id); }

    pub fn turn_started(&mut self) -> bool {
        if !self.transport_ready() { return false; }
        let mut candidate=self.integrity.clone(); if let Err(e)=candidate.turn_started(){return self.reject(e);}
        if self.publish(SemanticEvent::TurnStarted{context:self.proposed_context()}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn assistant_started(&mut self) -> bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if let Err(e)=candidate.assistant_started(){return self.reject(e);} if self.publish(SemanticEvent::AssistantStarted{context:self.proposed_context()}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready(){return false;} if let Err(e)=self.integrity.assistant_delta(){return self.reject(e);} if self.publish(SemanticEvent::AssistantDelta{context:self.proposed_context(),delta:delta.into()}){self.next_sequence+=1;true}else{false}
    }
    pub fn assistant_completed(&mut self) -> bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if let Err(e)=candidate.assistant_completed(){return self.reject(e);} if self.publish(SemanticEvent::AssistantCompleted{context:self.proposed_context()}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn assistant_yields_to_action(&mut self) -> bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if let Err(e)=candidate.assistant_yields_to_action(){return self.reject(e);} self.integrity=candidate; true
    }
    pub fn thinking_started(&mut self) -> bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if let Err(e)=candidate.thinking_started(){return self.reject(e);} if self.publish(SemanticEvent::ThinkingStarted{context:self.proposed_context()}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if !self.transport_ready(){return false;} if let Err(e)=self.integrity.thinking_delta(){return self.reject(e);} if self.publish(SemanticEvent::ThinkingDelta{context:self.proposed_context(),delta:delta.into()}){self.next_sequence+=1;true}else{false}
    }
    pub fn thinking_completed(&mut self) -> bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if let Err(e)=candidate.thinking_completed(){return self.reject(e);} if self.publish(SemanticEvent::ThinkingCompleted{context:self.proposed_context()}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn tool_call_requested(&mut self, upstream_id: impl Into<ToolCallId>, name: impl Into<String>) -> bool { self.tool_call_requested_with_ui(upstream_id,name,None) }
    pub fn tool_call_requested_with_ui(&mut self, upstream_id: impl Into<ToolCallId>, name: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready(){return false;} let upstream_id=upstream_id.into(); if upstream_id.is_empty(){return self.reject(IntegrityError::new("tool call identity must be non-empty"));}
        let semantic_id=match self.bind_tool_identity(upstream_id.clone()){Ok(id)=>id,Err(e)=>return self.reject(e)}; let mut candidate=self.integrity.clone(); if let Err(e)=candidate.tool_call_requested(semantic_id.as_str()){self.rollback_tool_binding(&upstream_id);return self.reject(e);}
        let event=SemanticEvent::ToolCallRequested{context:ToolEventContext{event:self.proposed_context(),tool_call_id:semantic_id},name:name.into(),ui}; if self.publish(event){self.integrity=candidate;self.next_sequence+=1;true}else{self.rollback_tool_binding(&upstream_id);false}
    }
    pub fn permission_requested(&mut self, upstream_id: impl Into<ToolCallId>) -> bool {
        if !self.transport_ready(){return false;} let upstream_id=upstream_id.into(); let Some(semantic_id)=self.resolve_tool_identity(&upstream_id).cloned() else{return self.reject(IntegrityError::new(format!("permission_requested references unknown upstream tool {upstream_id}")));}; let mut candidate=self.integrity.clone(); if let Err(e)=candidate.permission_requested(semantic_id.as_str()){return self.reject(e);} if self.publish(SemanticEvent::PermissionRequested{context:ToolEventContext{event:self.proposed_context(),tool_call_id:semantic_id}}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn tool_execution_started(&mut self, upstream_id: impl Into<ToolCallId>) -> bool { self.tool_execution_started_with_ui(upstream_id,None) }
    pub fn tool_execution_started_with_ui(&mut self, upstream_id: impl Into<ToolCallId>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready(){return false;} let upstream_id=upstream_id.into(); let Some(semantic_id)=self.resolve_tool_identity(&upstream_id).cloned() else{return self.reject(IntegrityError::new(format!("tool_execution_started references unknown upstream tool {upstream_id}")));}; let mut candidate=self.integrity.clone(); if let Err(e)=candidate.tool_execution_started(semantic_id.as_str()){return self.reject(e);} if self.publish(SemanticEvent::ToolExecutionStarted{context:ToolEventContext{event:self.proposed_context(),tool_call_id:semantic_id},ui}){self.integrity=candidate;self.next_sequence+=1;true}else{false}
    }
    pub fn tool_result_received(&mut self, upstream_id: impl Into<ToolCallId>, result: impl Into<String>) -> bool { self.tool_result_received_with_ui(upstream_id,result,None) }
    pub fn tool_result_received_with_ui(&mut self, upstream_id: impl Into<ToolCallId>, result: impl Into<String>, ui: Option<ToolUiModel>) -> bool {
        if !self.transport_ready(){return false;} let upstream_id=upstream_id.into(); let Some(semantic_id)=self.resolve_tool_identity(&upstream_id).cloned() else{return self.reject(IntegrityError::new(format!("tool_result_received references unknown upstream tool {upstream_id}")));}; let mut candidate=self.integrity.clone(); if let Err(e)=candidate.tool_result_received(semantic_id.as_str()){return self.reject(e);} let event=SemanticEvent::ToolResultReceived{context:ToolEventContext{event:self.proposed_context(),tool_call_id:semantic_id},result:result.into(),ui}; if self.publish(event){self.integrity=candidate;self.next_sequence+=1;self.release_tool_identity(&upstream_id);true}else{false}
    }
    fn finish_terminal(&mut self, event:&str)->bool {
        if !self.transport_ready(){return false;} let mut candidate=self.integrity.clone(); if event!="turn_completed"{let reason=candidate.terminal_reason_for(event); if candidate.thinking_active(){if let Err(e)=candidate.thinking_completed(){return self.reject(e);}} if candidate.assistant_active(){if let Err(e)=candidate.assistant_completed(){return self.reject(e);}} if let Err(e)=candidate.abort_open_tools(reason){return self.reject(e);}}
        if let Err(e)=candidate.finish_terminal_after_scopes(event){return self.reject(e);} let context=self.proposed_context(); let emitted=match event{"turn_cancelled"=>self.publish(SemanticEvent::TurnCancelled{context}),"turn_failed"=>self.publish(SemanticEvent::TurnFailed{context}),"turn_completed"=>self.publish(SemanticEvent::TurnCompleted{context}),_=>return self.reject(IntegrityError::new(format!("unknown terminal event {event}")))}; if emitted{self.integrity=candidate;self.next_sequence+=1;self.tool_bindings.clear();self.seen_tool_ids.clear();} emitted
    }
    pub fn turn_cancelled(&mut self)->bool{self.finish_terminal("turn_cancelled")}
    pub fn turn_failed(&mut self)->bool{self.finish_terminal("turn_failed")}
    pub fn turn_completed(&mut self)->bool{self.finish_terminal("turn_completed")}
    #[cfg(test)] pub fn bind_count(&self)->usize{self.tool_bindings.len()}
}
