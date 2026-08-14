use std::sync::Arc;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::{Cancellation, EncapsError};

/// Lifecycle of one ACP prompt execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    Pending,
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl TurnState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

struct Inner {
    state: TurnState,
    task: Option<JoinHandle<()>>,
    state_tx: watch::Sender<TurnState>,
}

/// Owns exactly one unit of ACP work and its cancellation/completion state.
///
/// `AcpTurn` is intentionally payload-agnostic. The ACP agent can execute its
/// existing `prompt::run_turn` closure inside it without coupling this crate
/// to ACP schema types or Gemini runtime state.
pub struct AcpTurn {
    inner: Arc<Mutex<Inner>>,
    cancellation: Cancellation,
}

/// Cloneable control surface for a turn.
#[derive(Clone)]
pub struct AcpTurnHandle {
    inner: Arc<Mutex<Inner>>,
    cancellation: Cancellation,
    state_rx: watch::Receiver<TurnState>,
}

impl AcpTurn {
    pub fn new() -> (Self, AcpTurnHandle) {
        let (state_tx, state_rx) = watch::channel(TurnState::Pending);
        let inner = Arc::new(Mutex::new(Inner {
            state: TurnState::Pending,
            task: None,
            state_tx,
        }));
        let cancellation = Cancellation::new();
        let handle = AcpTurnHandle {
            inner: inner.clone(),
            cancellation: cancellation.clone(),
            state_rx,
        };
        (Self { inner, cancellation }, handle)
    }

    /// Runs the turn exactly once. Completion is represented by the worker's
    /// `Result`; cancellation is represented separately by the shared signal.
    pub async fn start<F, Fut>(&self, work: F) -> Result<(), EncapsError>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let mut inner = self.inner.lock().await;
        if inner.state != TurnState::Pending {
            return Err(EncapsError::AlreadyRunning);
        }
        inner.state = TurnState::Running;
        let _ = inner.state_tx.send(TurnState::Running);

        let cancellation = self.cancellation.clone();
        let inner_ref = self.inner.clone();
        inner.task = Some(tokio::spawn(async move {
            let result = work(cancellation.clone()).await;
            let mut guard = inner_ref.lock().await;
            let next = if cancellation.is_cancelled() {
                TurnState::Cancelled
            } else {
                match result {
                    Ok(()) => TurnState::Completed,
                    Err(error) => {
                        tracing::error!(%error, "ACP turn failed");
                        TurnState::Failed
                    }
                }
            };
            guard.state = next;
            let _ = guard.state_tx.send(next);
            guard.task = None;
        }));
        Ok(())
    }

    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    pub async fn cancel(&self) -> Result<(), EncapsError> {
        let mut inner = self.inner.lock().await;
        if inner.state.is_terminal() {
            return Ok(());
        }
        self.cancellation.cancel();
        inner.state = TurnState::Cancelling;
        let _ = inner.state_tx.send(TurnState::Cancelling);
        Ok(())
    }
}

impl AcpTurnHandle {
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    pub async fn state(&self) -> TurnState {
        self.inner.lock().await.state
    }

    pub async fn cancel(&self) -> Result<(), EncapsError> {
        self.cancellation.cancel();
        let mut inner = self.inner.lock().await;
        if inner.state.is_terminal() {
            return Ok(());
        }
        inner.state = TurnState::Cancelling;
        let _ = inner.state_tx.send(TurnState::Cancelling);
        Ok(())
    }

    pub fn subscribe_state(&self) -> watch::Receiver<TurnState> {
        self.state_rx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn turn_completes() {
        let (turn, handle) = AcpTurn::new();
        turn.start(|_| async { Ok(()) }).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(handle.state().await, TurnState::Completed);
    }

    #[tokio::test]
    async fn turn_cancellation_is_visible() {
        let (turn, handle) = AcpTurn::new();
        turn.start(|cancellation| async move {
            let mut rx = cancellation.subscribe();
            rx.changed().await.map_err(|_| EncapsError::ChannelClosed)?;
            Ok(())
        }).await.unwrap();
        tokio::task::yield_now().await;
        handle.cancel().await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(handle.state().await, TurnState::Cancelled);
    }

    #[tokio::test]
    async fn turn_cannot_start_twice() {
        let (turn, _) = AcpTurn::new();
        turn.start(|_| async { Ok(()) }).await.unwrap();
        let error = turn.start(|_| async { Ok(()) }).await.unwrap_err();
        assert!(matches!(error, EncapsError::AlreadyRunning));
    }
}
