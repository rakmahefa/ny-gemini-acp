use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::{Cancellation, EncapsError, ThreadCommand, ThreadState};

const COMMAND_CAPACITY: usize = 32;

struct Inner {
    state: ThreadState,
    task: Option<JoinHandle<()>>,
    command_rx: Option<mpsc::Receiver<ThreadCommand>>,
    state_tx: watch::Sender<ThreadState>,
}

/// Owns the lifecycle and concurrency boundary of an ACP worker.
pub struct AcpThread {
    inner: Arc<Mutex<Inner>>,
    cancellation: Cancellation,
}

/// Cloneable control surface for an [`AcpThread`].
#[derive(Clone)]
pub struct AcpThreadHandle {
    inner: Arc<Mutex<Inner>>,
    commands: mpsc::Sender<ThreadCommand>,
    cancellation: Cancellation,
    state_rx: watch::Receiver<ThreadState>,
}

impl AcpThread {
    /// Creates a new thread owner and a cloneable control handle.
    pub fn new() -> (Self, AcpThreadHandle) {
        let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (state_tx, state_rx) = watch::channel(ThreadState::Created);
        let inner = Arc::new(Mutex::new(Inner {
            state: ThreadState::Created,
            task: None,
            command_rx: Some(command_rx),
            state_tx,
        }));
        let cancellation = Cancellation::new();

        let thread = Self {
            inner: inner.clone(),
            cancellation: cancellation.clone(),
        };
        let handle = AcpThreadHandle {
            inner,
            commands,
            cancellation,
            state_rx,
        };
        (thread, handle)
    }

    /// Starts the worker exactly once for this thread owner.
    pub async fn start<F, Fut>(&self, worker: F) -> Result<(), EncapsError>
    where
        F: FnOnce(mpsc::Receiver<ThreadCommand>, Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        if inner.state != ThreadState::Created {
            return Err(EncapsError::AlreadyRunning);
        }

        inner.state = ThreadState::Starting;
        let _ = inner.state_tx.send(ThreadState::Starting);
        let command_rx = inner.command_rx.take().ok_or(EncapsError::AlreadyRunning)?;
        let cancellation = self.cancellation.clone();
        let inner_ref = self.inner.clone();

        let task = tokio::spawn(async move {
            {
                let guard = inner_ref.lock().await;
                let _ = guard.state_tx.send(ThreadState::Running);
            }

            let result = worker(command_rx, cancellation).await;
            let mut guard = inner_ref.lock().await;
            let next = match result {
                Ok(()) => ThreadState::Stopped,
                Err(error) => {
                    tracing::error!(%error, "ACP encapsulated worker failed");
                    ThreadState::Failed
                }
            };
            guard.state = next;
            let _ = guard.state_tx.send(next);
            guard.task = None;
        });

        inner.task = Some(task);
        Ok(())
    }

    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    pub async fn stop(&self) -> Result<(), EncapsError> {
        self.cancellation.cancel();
        let mut inner = self.inner.lock().await;
        if inner.state.is_terminal() {
            return Ok(());
        }
        if inner.state == ThreadState::Created {
            inner.state = ThreadState::Stopped;
            let _ = inner.state_tx.send(ThreadState::Stopped);
            return Ok(());
        }
        inner.state = ThreadState::Stopping;
        let _ = inner.state_tx.send(ThreadState::Stopping);
        Ok(())
    }
}

impl AcpThreadHandle {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    pub async fn state(&self) -> ThreadState {
        self.inner.lock().await.state
    }

    pub async fn stop(&self) -> Result<(), EncapsError> {
        self.cancellation.cancel();
        self.commands
            .send(ThreadCommand::Stop)
            .await
            .map_err(|_| EncapsError::ChannelClosed)
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ThreadState> {
        self.state_rx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn starts_worker_and_reaches_stopped() {
        let (thread, handle) = AcpThread::new();
        thread
            .start(|mut commands, cancellation| async move {
                tokio::select! {
                    _ = cancellation.subscribe().changed() => Ok(()),
                    Some(ThreadCommand::Stop) = commands.recv() => Ok(()),
                }
            })
            .await
            .unwrap();

        assert_eq!(handle.state().await, ThreadState::Starting);
        tokio::task::yield_now().await;
        assert_eq!(handle.state().await, ThreadState::Running);

        handle.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(handle.state().await, ThreadState::Stopped);
    }

    #[tokio::test]
    async fn start_is_single_use() {
        let (thread, _) = AcpThread::new();
        thread
            .start(|_, _| async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(thread.start(|_, _| async { Ok(()) }).await, Err(EncapsError::AlreadyRunning));
    }

    #[tokio::test]
    async fn cancellation_is_shared() {
        let (thread, handle) = AcpThread::new();
        assert!(!thread.cancellation().is_cancelled());
        handle.cancellation().cancel();
        assert!(thread.cancellation().is_cancelled());
    }
}
