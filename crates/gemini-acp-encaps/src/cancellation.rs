use std::sync::Arc;
use tokio::sync::watch;

/// Cheap, clonable cancellation signal shared by a thread and its children.
#[derive(Clone)]
pub struct Cancellation {
    tx: Arc<watch::Sender<bool>>,
}

impl Cancellation {
    pub fn new() -> Self {
        let (tx, _) = watch::channel(false);
        Self { tx: Arc::new(tx) }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.tx.borrow()
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}
