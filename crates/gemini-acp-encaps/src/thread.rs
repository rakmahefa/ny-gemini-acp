use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::{Cancellation, EncapsError, ThreadCommand, ThreadState};

const COMMAND_CAPACITY: usize = 32;

struct CommandBus {
    sender: Mutex<mpsc::Sender<ThreadCommand>>,
    receiver: Mutex<Option<mpsc::Receiver<ThreadCommand>>>,
}

struct Inner {
    state: ThreadState,
    task: Option<JoinHandle<()>>,
}

pub struct AcpThread {
    inner: Arc<Mutex<Inner>>,
    commands: Arc<CommandBus>,
    cancellation: Cancellation,
}

#[derive(Clone)]
pub struct AcpThreadHandle {
    inner: Arc<Mutex<Inner>>,
    commands: Arc<CommandBus>,
    cancellation: Cancellation,
    state_rx: watch::Receiver<ThreadState>,
}

impl AcpThread {
    pub fn new() -> (Self, AcpThreadHandle) {
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let commands = Arc::new(CommandBus {
            sender: Mutex::new(sender),
            receiver: Mutex::new(Some(receiver)),
        });
        let (state_tx, state_rx) = watch::channel(ThreadState::Created);
        let inner = Arc::new(Mutex::new(Inner {
            state: ThreadState::Created,
            task: None,
        }));
        let cancellation = Cancellation::new();
        let thread = Self {
            inner: inner.clone(),
            commands: commands.clone(),
            cancellation: cancellation.clone(),
        };
        let handle = AcpThreadHandle {
            inner,
            commands,
            cancellation,
            state_rx,
        };
        let _ = state_tx;
        (thread, handle)
    }

    pub async fn start<F, Fut>(&self, worker: F) -> Result<(), EncapsError>
    where
        F: FnOnce(mpsc::Receiver<ThreadCommand>, Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        if !matches!(inner.state, ThreadState::Created | ThreadState::Stopped) {
            return Err(EncapsError::AlreadyRunning);
        }

        let command_rx = self
            .commands
            .receiver
            .lock()
            .await
            .take()
            .ok_or(EncapsError::ChannelClosed)?;

        inner.state = ThreadState::Starting;
        let cancellation = self.cancellation.clone();
        let inner_ref = self.inner.clone();
        let task = tokio::spawn(async move {
            {
                let mut guard = inner_ref.lock().await;
                guard.state = ThreadState::Running;
            }

            let result = worker(command_rx, cancellation).await;
            let mut guard = inner_ref.lock().await;
            guard.state = match result {
                Ok(()) => ThreadState::Stopped,
                Err(_) => ThreadState::Failed,
            };
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
        inner.state = ThreadState::Stopping;
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
        let sender = self.commands.sender.lock().await;
        sender
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
                let mut cancellation_rx = cancellation.subscribe();
                tokio::select! {
                    _ = cancellation_rx.changed() => Ok(()),
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
}