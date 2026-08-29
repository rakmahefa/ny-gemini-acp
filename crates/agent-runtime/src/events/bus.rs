use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, mpsc};

use super::SemanticEvent;

const DEFAULT_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SemanticEvent>,
    turn_senders: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<SemanticEvent>>>>,
}

impl Default for EventBus {
    fn default() -> Self { Self::new() }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_CAPACITY);
        Self { sender, turn_senders: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn publish_global(&self, event: SemanticEvent) -> usize {
        let subscribers = self.sender.send(event.clone()).unwrap_or(0);
        tracing::debug!(
            event = event_kind(&event),
            session = %event_session_id(&event),
            turn = %event_turn_id(&event),
            sequence = event_sequence(&event),
            tool_call_id = event_tool_call_id(&event).unwrap_or(""),
            subscribers,
            "published semantic event globally"
        );
        subscribers
    }

    pub fn has_turn_subscriber(&self, turn_id: &str) -> bool {
        self.turn_senders.lock().map(|senders| senders.contains_key(turn_id)).unwrap_or(false)
    }

    pub fn publish_turn(&self, event: SemanticEvent) -> Result<(), String> {
        let turn_id = event_turn_id(&event).to_owned();
        let sender = self.turn_senders.lock().ok().and_then(|senders| senders.get(&turn_id).cloned());
        let Some(sender) = sender else {
            tracing::warn!(event = event_kind(&event), turn = %turn_id, "semantic event rejected: turn transport is absent");
            return Err(format!("no ACP subscriber for turn {turn_id}"));
        };
        if sender.send(event.clone()).is_ok() {
            tracing::debug!(
                event = event_kind(&event),
                session = %event_session_id(&event),
                turn = %turn_id,
                sequence = event_sequence(&event),
                tool_call_id = event_tool_call_id(&event).unwrap_or(""),
                "delivered semantic event to turn transport"
            );
            Ok(())
        } else {
            if let Ok(mut senders) = self.turn_senders.lock() { senders.remove(&turn_id); }
            tracing::warn!(event = event_kind(&event), turn = %turn_id, "semantic event rejected: turn transport disconnected");
            Err(format!("ACP subscriber for turn {turn_id} disconnected"))
        }
    }

    pub fn publish(&self, event: SemanticEvent) -> Result<(), String> {
        self.publish_turn(event.clone())?;
        self.publish_global(event);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SemanticEvent> { self.sender.subscribe() }

    pub fn subscribe_turn(&self, turn_id: &str) -> mpsc::UnboundedReceiver<SemanticEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut senders = self.turn_senders.lock().expect("event bus turn sender registry poisoned");
        senders.insert(turn_id.to_owned(), sender);
        tracing::debug!(turn = turn_id, "registered turn transport subscriber");
        receiver
    }

    pub fn close_turn(&self, turn_id: &str) {
        if let Ok(mut senders) = self.turn_senders.lock() {
            senders.remove(turn_id);
        }
        tracing::debug!(turn = turn_id, "closed turn transport subscriber");
    }
}

fn event_turn_id(event: &SemanticEvent) -> &str {
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
        | SemanticEvent::TurnCompleted { context } => context.turn_id.as_str(),
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => context.event.turn_id.as_str(),
    }
}

fn event_session_id(event: &SemanticEvent) -> &str {
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
        | SemanticEvent::TurnCompleted { context } => context.session_id.as_str(),
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => context.event.session_id.as_str(),
    }
}

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

fn event_tool_call_id(event: &SemanticEvent) -> Option<&str> {
    match event {
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => Some(context.tool_call_id.as_str()),
        _ => None,
    }
}

fn event_kind(event: &SemanticEvent) -> &'static str {
    match event {
        SemanticEvent::TurnStarted { .. } => "turn_started",
        SemanticEvent::AssistantStarted { .. } => "assistant_started",
        SemanticEvent::AssistantDelta { .. } => "assistant_delta",
        SemanticEvent::AssistantCompleted { .. } => "assistant_completed",
        SemanticEvent::ThinkingStarted { .. } => "thinking_started",
        SemanticEvent::ThinkingDelta { .. } => "thinking_delta",
        SemanticEvent::ThinkingCompleted { .. } => "thinking_completed",
        SemanticEvent::ToolCallRequested { .. } => "tool_call_requested",
        SemanticEvent::PermissionRequested { .. } => "permission_requested",
        SemanticEvent::ToolExecutionStarted { .. } => "tool_execution_started",
        SemanticEvent::ToolResultReceived { .. } => "tool_result_received",
        SemanticEvent::TurnCancelled { .. } => "turn_cancelled",
        SemanticEvent::TurnFailed { .. } => "turn_failed",
        SemanticEvent::TurnCompleted { .. } => "turn_completed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventContext;

    fn event(turn_id: &str, sequence: u64) -> SemanticEvent {
        SemanticEvent::TurnStarted { context: EventContext::new("session", turn_id, sequence) }
    }

    #[tokio::test]
    async fn turn_subscribers_only_receive_their_turn() {
        let bus = EventBus::new();
        let mut turn_a = bus.subscribe_turn("turn-a");
        let mut turn_b = bus.subscribe_turn("turn-b");
        bus.publish_turn(event("turn-a", 0)).unwrap();
        bus.publish_turn(event("turn-b", 0)).unwrap();
        assert_eq!(event_turn_id(&turn_a.recv().await.unwrap()), "turn-a");
        assert_eq!(event_turn_id(&turn_b.recv().await.unwrap()), "turn-b");
    }

    #[tokio::test]
    async fn turn_transport_does_not_lag_under_a_burst() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_turn("burst");
        for sequence in 0..10_000u64 { bus.publish_turn(event("burst", sequence)).unwrap(); }
        for sequence in 0..10_000u64 {
            let received = receiver.recv().await.unwrap();
            assert_eq!(sequence, match received { SemanticEvent::TurnStarted { context } => context.sequence, _ => unreachable!() });
        }
    }

    #[test]
    fn global_broadcast_is_best_effort_without_subscribers() { assert_eq!(EventBus::new().publish_global(event("turn", 0)), 0); }

    #[test]
    fn turn_transport_is_mandatory() {
        let bus = EventBus::new();
        assert!(bus.publish_turn(event("turn", 0)).is_err());
        assert!(!bus.has_turn_subscriber("turn"));
    }

    #[test]
    fn closing_a_turn_removes_only_its_dedicated_sender() {
        let bus = EventBus::new();
        let _turn_a = bus.subscribe_turn("turn-a");
        let _turn_b = bus.subscribe_turn("turn-b");
        bus.close_turn("turn-a");
        assert!(bus.publish_turn(event("turn-a", 0)).is_err());
        bus.close_turn("turn-b");
        let mut turn_b = bus.subscribe_turn("turn-b");
        bus.publish_turn(event("turn-b", 1)).unwrap();
        assert_eq!(event_turn_id(&turn_b.try_recv().unwrap()), "turn-b");
    }

    #[tokio::test]
    async fn global_and_turn_subscribers_can_receive_the_same_event() {
        let bus = EventBus::new();
        let mut global = bus.subscribe();
        let mut turn = bus.subscribe_turn("turn");
        bus.publish(event("turn", 0)).unwrap();
        assert_eq!(event_turn_id(&global.recv().await.unwrap()), "turn");
        assert_eq!(event_turn_id(&turn.recv().await.unwrap()), "turn");
    }
}
