use tokio::sync::broadcast;

use super::AcpSemanticEvent;

const DEFAULT_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AcpSemanticEvent>,
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
        event: AcpSemanticEvent,
    ) -> Result<usize, broadcast::error::SendError<AcpSemanticEvent>> {
        self.sender.send(event)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AcpSemanticEvent> {
        self.sender.subscribe()
    }
}
