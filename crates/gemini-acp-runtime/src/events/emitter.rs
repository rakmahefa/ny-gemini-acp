use super::{AcpSemanticEvent, EventBus, EventContext, ToolEventContext};
use super::integrity::{IntegrityError, TurnIntegrity, TurnPhase};

/// Owns and validates the semantic sequence for one turn.
/// Invalid transitions never reach the event bus and do not consume a sequence number.
#[derive(Clone)]
pub struct TurnEventEmitter {
    bus: EventBus,
    session_id: String,
    turn_id: String,
    next_sequence: u64,
    integrity: TurnIntegrity,
}

impl TurnEventEmitter {
    pub fn new(bus: EventBus, session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self { bus, session_id: session_id.into(), turn_id: turn_id.into(), next_sequence: 0, integrity: TurnIntegrity::default() }
    }

    pub fn sequence(&self) -> u64 { self.next_sequence }
    pub fn phase(&self) -> TurnPhase { self.integrity.phase() }

    fn context(&mut self) -> EventContext {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        EventContext::new(self.session_id.clone(), self.turn_id.clone(), sequence)
    }

    fn tool_context(&mut self, tool_call_id: impl Into<String>) -> ToolEventContext {
        ToolEventContext { event: self.context(), tool_call_id: tool_call_id.into() }
    }

    fn reject(&self, error: IntegrityError) -> bool {
        tracing::error!(session=%self.session_id, turn=%self.turn_id, error=%error.message, "rejected invalid semantic event transition");
        false
    }

    fn publish(&self, event: AcpSemanticEvent) {
        if let Err(error) = self.bus.publish(event) { tracing::debug!(?error, "semantic event has no active subscribers"); }
    }

    pub fn turn_started(&mut self) -> bool {
        if let Err(e)=self.integrity.turn_started(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::TurnStarted{context}); true
    }
    pub fn assistant_started(&mut self) -> bool {
        if let Err(e)=self.integrity.assistant_started(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::AssistantStarted{context}); true
    }
    pub fn assistant_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e)=self.integrity.assistant_delta(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::AssistantDelta{context,delta:delta.into()}); true
    }
    pub fn assistant_completed(&mut self) -> bool {
        if let Err(e)=self.integrity.assistant_completed(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::AssistantCompleted{context}); true
    }
    pub fn thinking_started(&mut self) -> bool {
        if let Err(e)=self.integrity.thinking_started(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::ThinkingStarted{context}); true
    }
    pub fn thinking_delta(&mut self, delta: impl Into<String>) -> bool {
        if let Err(e)=self.integrity.thinking_delta(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::ThinkingDelta{context,delta:delta.into()}); true
    }
    pub fn thinking_completed(&mut self) -> bool {
        if let Err(e)=self.integrity.thinking_completed(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::ThinkingCompleted{context}); true
    }
    pub fn tool_call_requested(&mut self, id: impl Into<String>, name: impl Into<String>) -> bool {
        let id=id.into(); if let Err(e)=self.integrity.tool_call_requested(&id){return self.reject(e)}; let context=self.tool_context(id); self.publish(AcpSemanticEvent::ToolCallRequested{context,name:name.into()}); true
    }
    pub fn permission_requested(&mut self, id: impl Into<String>) -> bool {
        let id=id.into(); if let Err(e)=self.integrity.permission_requested(&id){return self.reject(e)}; let context=self.tool_context(id); self.publish(AcpSemanticEvent::PermissionRequested{context}); true
    }
    pub fn tool_execution_started(&mut self, id: impl Into<String>) -> bool {
        let id=id.into(); if let Err(e)=self.integrity.tool_execution_started(&id){return self.reject(e)}; let context=self.tool_context(id); self.publish(AcpSemanticEvent::ToolExecutionStarted{context}); true
    }
    pub fn tool_result_received(&mut self, id: impl Into<String>, result: impl Into<String>) -> bool {
        let id=id.into(); if let Err(e)=self.integrity.tool_result_received(&id){return self.reject(e)}; let context=self.tool_context(id); self.publish(AcpSemanticEvent::ToolResultReceived{context,result:result.into()}); true
    }
    pub fn turn_cancelled(&mut self) -> bool {
        if let Err(e)=self.integrity.turn_cancelled(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::TurnCancelled{context}); true
    }
    pub fn turn_completed(&mut self) -> bool {
        if let Err(e)=self.integrity.turn_completed(){return self.reject(e)}; let context=self.context(); self.publish(AcpSemanticEvent::TurnCompleted{context}); true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seq(event:&AcpSemanticEvent)->u64 { match event {
        AcpSemanticEvent::TurnStarted{context}|AcpSemanticEvent::AssistantStarted{context}|AcpSemanticEvent::AssistantCompleted{context}|AcpSemanticEvent::ThinkingStarted{context}|AcpSemanticEvent::ThinkingCompleted{context}|AcpSemanticEvent::TurnCancelled{context}|AcpSemanticEvent::TurnCompleted{context}=>context.sequence,
        AcpSemanticEvent::AssistantDelta{context,..}|AcpSemanticEvent::ThinkingDelta{context,..}=>context.sequence,
        AcpSemanticEvent::ToolCallRequested{context,..}|AcpSemanticEvent::PermissionRequested{context}|AcpSemanticEvent::ToolExecutionStarted{context}|AcpSemanticEvent::ToolResultReceived{context,..}=>context.event.sequence,
    }}

    #[tokio::test]
    async fn accepted_events_have_contiguous_sequences() {
        let bus=EventBus::new(); let mut rx=bus.subscribe(); let mut e=TurnEventEmitter::new(bus,"s","t");
        assert!(e.turn_started()); assert!(e.assistant_started()); assert!(e.thinking_started()); assert!(e.thinking_delta("x")); assert!(e.thinking_completed()); assert!(e.assistant_delta("y")); assert!(e.assistant_completed());
        assert!(e.tool_call_requested("c","shell_exec")); assert!(e.permission_requested("c")); assert!(e.tool_execution_started("c")); assert!(e.tool_result_received("c","ok")); assert!(e.turn_completed());
        let events:Vec<_>=std::iter::from_fn(||rx.try_recv().ok()).collect(); assert_eq!(events.iter().map(seq).collect::<Vec<_>>(),(0..12).collect::<Vec<_>>()); assert_eq!(e.sequence(),12);
    }

    #[tokio::test]
    async fn invalid_events_are_rejected_without_sequence_consumption() {
        let bus=EventBus::new(); let mut rx=bus.subscribe(); let mut e=TurnEventEmitter::new(bus,"s","t");
        assert!(!e.turn_completed()); assert_eq!(e.sequence(),0); assert!(rx.try_recv().is_err());
        assert!(e.turn_started()); assert!(!e.turn_started()); assert_eq!(e.sequence(),1); assert_eq!(seq(&rx.try_recv().unwrap()),0); assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn cancellation_is_terminal_even_with_open_lifecycle() {
        let bus=EventBus::new(); let mut rx=bus.subscribe(); let mut e=TurnEventEmitter::new(bus,"s","t");
        assert!(e.turn_started()); assert!(e.assistant_started()); assert!(e.thinking_started()); assert!(e.tool_call_requested("c","shell_exec")); assert!(e.turn_cancelled()); assert!(!e.turn_completed()); assert!(!e.assistant_delta("late"));
        let events:Vec<_>=std::iter::from_fn(||rx.try_recv().ok()).collect(); assert!(matches!(events.last(),Some(AcpSemanticEvent::TurnCancelled{..}))); assert_eq!(events.len(),5);
    }
}
