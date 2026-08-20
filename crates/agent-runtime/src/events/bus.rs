use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

use super::SemanticEvent;

const DEFAULT_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SemanticEvent>,
    turn_senders: Arc<Mutex<HashMap<String, broadcast::Sender<SemanticEvent>>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_CAPACITY);
        Self {
            sender,
            turn_senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn publish(
        &self,
        event: SemanticEvent,
    ) -> Result<usize, Box<broadcast::error::SendError<SemanticEvent>>> {
        let turn_id = event_turn_id(&event).to_owned();
        let error_event = event.clone();
        let mut delivered = self.sender.send(event.clone()).unwrap_or(0);

        let turn_sender = self
            .turn_senders
            .lock()
            .ok()
            .and_then(|senders| senders.get(&turn_id).cloned());

        if let Some(sender) = turn_sender {
            match sender.send(event) {
                Ok(receivers) => delivered = delivered.saturating_add(receivers),
                Err(_) => {
                    if let Ok(mut senders) = self.turn_senders.lock() {
                        senders.remove(&turn_id);
                    }
                }
            }
        }

        if delivered == 0 {
            Err(Box::new(broadcast::error::SendError(error_event)))
        } else {
            Ok(delivered)
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SemanticEvent> {
        self.sender.subscribe()
    }

    /// Subscribe to one turn only.
    ///
    /// Each turn has an independent bounded buffer so unrelated concurrent turns
    /// cannot make this receiver lag behind. Transport integrity is then enforced
    /// by the consumer through the per-turn sequence numbers.
    pub fn subscribe_turn(&self, turn_id: &str) -> broadcast::Receiver<SemanticEvent> {
        let mut senders = self
            .turn_senders
            .lock()
            .expect("event bus turn sender registry poisoned");
        let sender = senders
            .entry(turn_id.to_owned())
            .or_insert_with(|| broadcast::channel(DEFAULT_CAPACITY).0);
        sender.subscribe()
    }

    pub fn close_turn(&self, turn_id: &str) {
        if let Ok(mut senders) = self.turn_senders.lock() {
            senders.remove(turn_id);
        }
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
        | SemanticEvent::TurnCompleted { context } => &context.turn_id,
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => &context.event.turn_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventContext;

    fn event(turn_id: &str, sequence: u64) -> SemanticEvent {
        SemanticEvent::TurnStarted {
            context: EventContext::new("session", turn_id, sequence),
        }
    }

    #[tokio::test]
    async fn turn_subscribers_only_receive_their_turn() {
        let bus = EventBus::new();
        let mut turn_a = bus.subscribe_turn("turn-a");
        let mut turn_b = bus.subscribe_turn("turn-b");

        bus.publish(event("turn-a", 0)).expect("turn-a has a receiver");
        bus.publish(event("turn-b", 0)).expect("turn-b has a receiver");

        let a = turn_a.recv().await.unwrap();
        let b = turn_b.recv().await.unwrap();
        assert_eq!(event_turn_id(&a), "turn-a");
        assert_eq!(event_turn_id(&b), "turn-b");
    }

    #[tokio::test]
    async fn closing_a_turn_removes_only_its_dedicated_sender() {
        let bus = EventBus::new();
        let _turn_a = bus.subscribe_turn("turn-a");
        let _turn_b = bus.subscribe_turn("turn-b");

        bus.close_turn("turn-a");
        bus.publish(event("turn-a", 0)).err();
        bus.close_turn("turn-b");
        let mut turn_b = bus.subscribe_turn("turn-b");
        bus.publish(event("turn-b", 1)).expect("turn-b has a new receiver");
        assert_eq!(event_turn_id(&turn_b.recv().await.unwrap()), "turn-b");
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
