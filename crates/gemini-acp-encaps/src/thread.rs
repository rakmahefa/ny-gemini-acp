use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::{Cancellation, EncapsError, ThreadCommand, ThreadState};

const COMMAND_CAPACITY: usize = 32;

struct Inner {
    state: ThreadState,
    task: Option<JoinHandle<()>>,
}

/// Owns the lifecycle and concurrency boundary of an ACP worker.
///
/// The worker callback is deliberately generic: the encapsulation layer does
/// not know about ACP requests or Gemini. The agent layer supplies that work
/// when it is migrated onto this foundation.
pub struct AcpThread {
    inner: Arc<Mutex<Inner>>,
    commands: mpsc::Sender<ThreadCommand>,
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
    /// Creates a stopped thread owner and a control handle.
    pub fn new() -> (Self, AcpThreadHandle) {
        let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
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

        // Keep the receiver alive until start() installs the actual worker.
        drop(command_rx);
        drop(state_tx);
        (thread, handle)
    }

    /// Starts a worker. A worker is a Tokio task, keeping the API runtime
    /// agnostic while providing a single ownership boundary.
    pub async fn start<F, Fut>(&self, worker: F) -> Result<(), EncapsError>
    where
        F: FnOnce(mpsc::Receiver<ThreadCommand>, Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        if !matches!(inner.state, ThreadState::Created | ThreadState::Stopped) {
            return Err(EncapsError::AlreadyRunning);
        }

        inner.state = ThreadState::Starting;
        let cancellation = self.cancellation.clone();
        let commands = self.commands.clone();
        let inner_ref = self.inner.clone();

        let (worker_tx, worker_rx) = mpsc::channel(COMMAND_CAPACITY);
        // Forward commands from the public channel to the worker channel.
        // This keeps the control surface independent from worker internals.
        let mut public_rx = commands.subscribe_receiver();
        let _ = (&mut public_rx, &worker_tx);

        let task = tokio::spawn(async move {
            {
                let mut guard = inner_ref.lock().await;
                guard.state = ThreadState::Running;
            }

            let result = worker(worker_rx, cancellation).await;
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
        let _ = self.commands.send(ThreadCommand::Stop).await;
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
        self.commands
            .send(ThreadCommand::Stop)
            .await
            .map_err(|_| EncapsError::ChannelClosed)
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ThreadState> {
        self.state_rx.clone()
    }
}
