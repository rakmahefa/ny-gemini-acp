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
        self.tx.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.tx.borrow()
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }

    /// Wait until this cancellation source is cancelled.
    ///
    /// The check before `changed().await` closes the race where cancellation
    /// happens before the waiter subscribes.
    pub async fn cancelled(&self) {
        let mut receiver = self.subscribe();
        loop {
            if *receiver.borrow() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Cancellation;

    #[tokio::test]
    async fn cancelled_returns_when_already_cancelled() {
        let cancellation = Cancellation::new();
        cancellation.cancel();
        cancellation.cancelled().await;
        assert!(cancellation.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_waits_for_signal() {
        let cancellation = Cancellation::new();
        let waiter = cancellation.clone();
        let task = tokio::spawn(async move { waiter.cancelled().await });
        tokio::task::yield_now().await;
        cancellation.cancel();
        task.await.unwrap();
    }
}
