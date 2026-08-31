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

    /// D-11 : stratégie de verrou unique dans ce fichier — un empoisonnement
    /// (panic ailleurs sous lock) est récupéré au lieu de paniquer à son tour
    /// (`subscribe_turn` utilisait `.expect`) ou de sauter silencieusement
    /// l'opération (`close_turn`, `publish_turn`).
    fn lock_senders(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, mpsc::UnboundedSender<SemanticEvent>>> {
        self.turn_senders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn publish_global(&self, event: SemanticEvent) -> usize {
        let subscribers = self.sender.send(event.clone()).unwrap_or(0);
        tracing::debug!(
            event = event.kind(),
            session = %event.session_id(),
            turn = %event.turn_id(),
            sequence = event.sequence(),
            tool_call_id = event.tool_call_id().unwrap_or(""),
            subscribers,
            "published semantic event globally"
        );
        subscribers
    }

    pub fn has_turn_subscriber(&self, turn_id: &str) -> bool {
        self.lock_senders().contains_key(turn_id)
    }

    pub fn publish_turn(&self, event: SemanticEvent) -> Result<(), String> {
        let turn_id = event.turn_id().to_owned();
        let sender = self.lock_senders().get(&turn_id).cloned();

        let Some(sender) = sender else {
            tracing::warn!(
                event = event.kind(),
                turn = %turn_id,
                "semantic event rejected: turn transport is absent"
            );
            return Err(format!("no ACP subscriber for turn {turn_id}"));
        };

        if sender.send(event.clone()).is_ok() {
            tracing::debug!(
                event = event.kind(),
                session = %event.session_id(),
                turn = %turn_id,
                sequence = event.sequence(),
                tool_call_id = event.tool_call_id().unwrap_or(""),
                "delivered semantic event to turn transport"
            );
            Ok(())
        } else {
            self.lock_senders().remove(&turn_id);
            tracing::warn!(
                event = event.kind(),
                turn = %turn_id,
                "semantic event rejected: turn transport disconnected"
            );
            Err(format!("ACP subscriber for turn {turn_id} disconnected"))
        }
    }

    /// Compatibility helper: mandatory turn delivery occurs before best-effort global fan-out.
    /// This preserves the invariant that a successful return means the protocol transport
    /// accepted the event before diagnostic consumers observe it.
    pub fn publish(&self, event: SemanticEvent) -> Result<(), String> {
        self.publish_turn(event.clone())?;
        self.publish_global(event);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SemanticEvent> {
        self.sender.subscribe()
    }

    pub fn subscribe_turn(&self, turn_id: &str) -> mpsc::UnboundedReceiver<SemanticEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.lock_senders().insert(turn_id.to_owned(), sender);
        tracing::debug!(turn = turn_id, "registered turn transport subscriber");
        receiver
    }

    pub fn close_turn(&self, turn_id: &str) {
        self.lock_senders().remove(turn_id);
        tracing::debug!(turn = turn_id, "closed turn transport subscriber");
    }
}#[cfg(test)]
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
        bus.publish_turn(event("turn-a", 0)).unwrap();
        bus.publish_turn(event("turn-b", 0)).unwrap();
        assert_eq!(turn_a.recv().await.unwrap().turn_id().to_string(), "turn-a");
        assert_eq!(turn_b.recv().await.unwrap().turn_id(), "turn-b");
    }

    #[tokio::test]
    async fn turn_transport_does_not_lag_under_a_burst() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_turn("burst");
        for sequence in 0..10_000u64 {
            bus.publish_turn(event("burst", sequence)).unwrap();
        }
        for sequence in 0..10_000u64 {
            let received = receiver.recv().await.unwrap();
            assert_eq!(
                sequence,
                match received {
                    SemanticEvent::TurnStarted { context } => context.sequence,
                    _ => unreachable!(),
                }
            );
        }
    }

    #[test]
    fn global_broadcast_is_best_effort_without_subscribers() {
        let bus = EventBus::new();
        assert_eq!(bus.publish_global(event("turn", 0)), 0);
    }

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
        assert_eq!(turn_b.try_recv().unwrap().turn_id(), "turn-b");
    }

    #[tokio::test]
    async fn global_and_turn_subscribers_can_receive_the_same_event() {
        let bus = EventBus::new();
        let mut global = bus.subscribe();
        let mut turn = bus.subscribe_turn("turn");
        let value = event("turn", 0);
        bus.publish(value).unwrap();
        assert_eq!(global.recv().await.unwrap().turn_id(), "turn");
        assert_eq!(turn.recv().await.unwrap().turn_id(), "turn");
    }
}
