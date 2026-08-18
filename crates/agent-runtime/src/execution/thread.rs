use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::{Cancellation, RuntimeError, ThreadCommand, ThreadState};

const COMMAND_CAPACITY: usize = 32;

struct CommandBus {
    sender: mpsc::Sender<ThreadCommand>,
    receiver: Mutex<Option<mpsc::Receiver<ThreadCommand>>>,
}

struct Inner {
    state: ThreadState,
    task: Option<JoinHandle<()>>,
    state_tx: watch::Sender<ThreadState>,
}

pub struct AgentThread {
    inner: Arc<Mutex<Inner>>,
    commands: Arc<CommandBus>,
    cancellation: Cancellation,
}

#[derive(Clone)]
pub struct AgentThreadHandle {
    inner: Arc<Mutex<Inner>>,
    cancellation: Cancellation,
    state_rx: watch::Receiver<ThreadState>,
}

impl AgentThread {
    /// Creates a new single-use agent thread and its control handle.
    ///
    /// An `AgentThread` may be started at most once. After the worker reaches a
    /// terminal state it cannot be restarted because its command receiver and
    /// cancellation domain belong to that execution instance.
    pub fn new() -> (Self, AgentThreadHandle) {
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let commands = Arc::new(CommandBus {
            sender,
            receiver: Mutex::new(Some(receiver)),
        });
        let (state_tx, state_rx) = watch::channel(ThreadState::Created);
        let inner = Arc::new(Mutex::new(Inner {
            state: ThreadState::Created,
            task: None,
            state_tx,
        }));
        let cancellation = Cancellation::new();
        let thread = Self {
            inner: inner.clone(),
            commands: commands.clone(),
            cancellation: cancellation.clone(),
        };
        let handle = AgentThreadHandle {
            inner,
            cancellation,
            state_rx,
        };
        (thread, handle)
    }

    /// Starts the worker exactly once.
    ///
    /// `Created` is the only state from which start is valid. A completed or
    /// failed thread is terminal and must not be reused for another worker.
    pub async fn start<F, Fut>(&self, worker: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(mpsc::Receiver<ThreadCommand>, Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), RuntimeError>> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        if inner.state != ThreadState::Created {
            return Err(RuntimeError::AlreadyStarted);
        }

        let command_rx = self
            .commands
            .receiver
            .lock()
            .await
            .take()
            .ok_or(RuntimeError::ChannelClosed)?;
        let command_tx = self.commands.sender.clone();

        inner.state = ThreadState::Starting;
        let _ = inner.state_tx.send(ThreadState::Starting);
        let cancellation = self.cancellation.clone();
        let inner_ref = self.inner.clone();
        let task = tokio::spawn(async move {
            let _command_tx = command_tx;
            {
                let mut guard = inner_ref.lock().await;
                guard.state = ThreadState::Running;
                let _ = guard.state_tx.send(ThreadState::Running);
            }

            let result = worker(command_rx, cancellation).await;
            let mut guard = inner_ref.lock().await;
            let state = match result {
                Ok(()) => ThreadState::Stopped,
                Err(_) => ThreadState::Failed,
            };
            guard.state = state;
            let _ = guard.state_tx.send(state);
            guard.task = None;
        });
        inner.task = Some(task);
        Ok(())
    }

    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    /// Requests shutdown. Repeated calls are harmless once shutdown has begun.
    /// The worker owns the final transition to `Stopped` or `Failed`.
    pub async fn stop(&self) -> Result<(), RuntimeError> {
        let mut inner = self.inner.lock().await;
        if inner.state.is_terminal() || inner.state == ThreadState::Stopping {
            return Ok(());
        }
        if inner.state == ThreadState::Created {
            return Ok(());
        }
        inner.state = ThreadState::Stopping;
        let _ = inner.state_tx.send(ThreadState::Stopping);
        self.cancellation.cancel();
        Ok(())
    }
}

impl AgentThreadHandle {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    pub async fn state(&self) -> ThreadState {
        self.inner.lock().await.state
    }

    /// Requests shutdown through the same cancellation path as `AgentThread`.
    /// Calling `stop` multiple times is idempotent.
    pub async fn stop(&self) -> Result<(), RuntimeError> {
        let mut inner = self.inner.lock().await;
        if inner.state.is_terminal() || inner.state == ThreadState::Stopping {
            return Ok(());
        }
        if inner.state == ThreadState::Created {
            return Ok(());
        }
        inner.state = ThreadState::Stopping;
        let _ = inner.state_tx.send(ThreadState::Stopping);
        self.cancellation.cancel();
        Ok(())
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ThreadState> {
        self.state_rx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn wait_for_state(handle: &AgentThreadHandle, expected: ThreadState) {
        let mut rx = handle.subscribe_state();
        while *rx.borrow() != expected {
            rx.changed().await.unwrap();
        }
    }

    #[tokio::test]
    async fn starts_worker_and_reaches_stopped() {
        let (thread, handle) = AgentThread::new();
        thread
            .start(|mut commands, cancellation| async move {
                let mut cancellation_rx = cancellation.subscribe();
                tokio::select! {
                    _ = cancellation_rx.changed() => Ok(()),
                    Some(ThreadCommand::Stop) = commands.recv() => Ok(()),
                }
            })
            .await
            .unwrap();
        wait_for_state(&handle, ThreadState::Running).await;
        assert_eq!(handle.state().await, ThreadState::Running);
        handle.stop().await.unwrap();
        wait_for_state(&handle, ThreadState::Stopped).await;
        assert_eq!(handle.state().await, ThreadState::Stopped);
    }

    #[tokio::test]
    async fn start_is_single_use_even_after_stop() {
        let (thread, handle) = AgentThread::new();
        thread.start(|_, _| async { Ok(()) }).await.unwrap();
        wait_for_state(&handle, ThreadState::Stopped).await;
        let result = thread.start(|_, _| async { Ok(()) }).await;
        assert!(matches!(result, Err(RuntimeError::AlreadyStarted)));
    }

    #[tokio::test]
    async fn stop_is_idempotent_and_concurrent() {
        let (thread, handle) = AgentThread::new();
        thread
            .start(|_, cancellation| async move {
                let mut rx = cancellation.subscribe();
                rx.changed().await.map_err(|_| RuntimeError::ChannelClosed)?;
                Ok(())
            })
            .await
            .unwrap();
        wait_for_state(&handle, ThreadState::Running).await;

        let first = handle.clone();
        let second = handle.clone();
        let (a, b) = tokio::join!(first.stop(), second.stop());
        assert!(a.is_ok());
        assert!(b.is_ok());
        wait_for_state(&handle, ThreadState::Stopped).await;
    }

    #[tokio::test]
    async fn stop_does_not_require_command_receiver() {
        let (thread, handle) = AgentThread::new();
        let worker_starts = Arc::new(AtomicUsize::new(0));
        let starts = worker_starts.clone();
        thread
            .start(move |_, cancellation| async move {
                starts.fetch_add(1, Ordering::SeqCst);
                let mut rx = cancellation.subscribe();
                rx.changed().await.map_err(|_| RuntimeError::ChannelClosed)?;
                Ok(())
            })
            .await
            .unwrap();
        wait_for_state(&handle, ThreadState::Running).await;
        handle.stop().await.unwrap();
        wait_for_state(&handle, ThreadState::Stopped).await;
        assert_eq!(worker_starts.load(Ordering::SeqCst), 1);
    }
}
