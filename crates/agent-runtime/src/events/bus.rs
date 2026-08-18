use tokio::sync::broadcast;

use super::SemanticEvent;

const DEFAULT_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SemanticEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_CAPACITY);
        Self { sender }
    }

    pub fn publish(
        &self,
        event: SemanticEvent,
    ) -> Result<usize, broadcast::error::SendError<SemanticEvent>> {
        self.sender.send(event)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SemanticEvent> {
        self.sender.subscribe()
    }
}
