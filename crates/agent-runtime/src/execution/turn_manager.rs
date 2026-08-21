use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::{AgentTurn, AgentTurnHandle, Cancellation, RuntimeError, TurnState};

const CANCEL_WAIT: Duration = Duration::from_secs(5);
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// Owns runtime turn concurrency, cancellation and active-turn handles.
/// Cross-process ownership is deliberately left to `Store`'s busy sentinel.
#[derive(Clone, Default)]
pub struct TurnManager {
    active: Arc<Mutex<HashMap<String, AgentTurnHandle>>>,
}

impl TurnManager {
    pub fn new() -> Self { Self::default() }

    pub async fn start<F, Fut>(&self, session_id: impl Into<String>, work: F) -> Result<AgentTurnHandle, RuntimeError>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), RuntimeError>> + Send + 'static,
    {
        let session_id = session_id.into();
        let (turn, handle) = AgentTurn::new();
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&session_id) { return Err(RuntimeError::TurnAlreadyActive); }
            active.insert(session_id.clone(), handle.clone());
        }

        let active = self.active.clone();
        let key = session_id.clone();
        let start_result = turn.start(move |cancellation| async move {
            let result = work(cancellation.clone()).await;
            active.lock().await.remove(&key);
            result
        }).await;
        if start_result.is_err() { self.active.lock().await.remove(&session_id); }
        start_result.map(|()| handle)
    }

    pub async fn cancel(&self, session_id: &str) -> Result<bool, RuntimeError> {
        let handle = self.active.lock().await.get(session_id).cloned();
        if let Some(handle) = handle { handle.cancel().await?; return Ok(true); }
        Ok(false)
    }

    /// Cancels a turn and waits until its worker has actually left the active set.
    pub async fn cancel_and_wait(&self, session_id: &str) -> Result<bool, RuntimeError> {
        if !self.cancel(session_id).await? { return Ok(false); }
        let deadline = Instant::now() + CANCEL_WAIT;
        while Instant::now() < deadline {
            if self.state(session_id).await.is_none() { return Ok(true); }
            tokio::time::sleep(CANCEL_POLL).await;
        }
        Err(RuntimeError::Task(format!("timeout cancelling turn for session {session_id}")))
    }

    pub async fn cancel_all(&self) -> Result<usize, RuntimeError> {
        let handles = self.active.lock().await.values().cloned().collect::<Vec<_>>();
        let mut cancelled = 0usize;
        for handle in handles { handle.cancel().await?; cancelled = cancelled.saturating_add(1); }
        Ok(cancelled)
    }

    /// Cancels every active turn and waits for all workers to leave the active set.
    pub async fn cancel_all_and_wait(&self) -> Result<usize, RuntimeError> {
        let session_ids = self.active.lock().await.keys().cloned().collect::<Vec<_>>();
        let mut cancelled = 0usize;
        for session_id in session_ids {
            if self.cancel_and_wait(&session_id).await? { cancelled = cancelled.saturating_add(1); }
        }
        Ok(cancelled)
    }

    pub async fn state(&self, session_id: &str) -> Option<TurnState> {
        let handle = self.active.lock().await.get(session_id).cloned();
        match handle { Some(handle) => Some(handle.state().await), None => None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::Notify;

    async fn wait_for_terminal(handle: &AgentTurnHandle) {
        let mut rx = handle.subscribe_state();
        while !rx.borrow().is_terminal() { rx.changed().await.unwrap(); }
    }

    #[tokio::test]
    async fn concurrent_starts_have_exactly_one_winner() {
        let manager = TurnManager::new();
        let barrier = Arc::new(Notify::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let m1 = manager.clone(); let m2 = manager.clone();
        let b1 = barrier.clone(); let b2 = barrier.clone(); let s1 = starts.clone(); let s2 = starts.clone();
        let first = tokio::spawn(async move { m1.start("session", move |cancellation| async move { s1.fetch_add(1, Ordering::SeqCst); let mut rx = cancellation.subscribe(); tokio::select! { _ = b1.notified() => Ok(()), _ = rx.changed() => Ok(()) } }).await });
        let second = tokio::spawn(async move { m2.start("session", move |cancellation| async move { s2.fetch_add(1, Ordering::SeqCst); let mut rx = cancellation.subscribe(); tokio::select! { _ = b2.notified() => Ok(()), _ = rx.changed() => Ok(()) } }).await });
        let first = first.await.unwrap(); let second = second.await.unwrap();
        let successes = [first.as_ref(), second.as_ref()].into_iter().filter(|r| r.is_ok()).count();
        let failures = [first.as_ref(), second.as_ref()].into_iter().filter(|r| matches!(r, Err(RuntimeError::TurnAlreadyActive))).count();
        assert_eq!((successes, failures, starts.load(Ordering::SeqCst)), (1, 1, 1));
        if let Ok(handle) = first { handle.cancel().await.unwrap(); wait_for_terminal(&handle).await; }
        if let Ok(handle) = second { handle.cancel().await.unwrap(); wait_for_terminal(&handle).await; }
        barrier.notify_waiters();
    }

    #[tokio::test]
    async fn cancel_and_wait_removes_turn_before_return() {
        let manager = TurnManager::new();
        let handle = manager.start("session", |cancellation| async move { cancellation.subscribe().changed().await.map_err(|_| RuntimeError::ChannelClosed)?; Ok(()) }).await.unwrap();
        assert!(manager.cancel_and_wait("session").await.unwrap());
        assert!(manager.state("session").await.is_none());
        assert_eq!(handle.state().await, TurnState::Cancelled);
    }

    #[tokio::test]
    async fn cancel_all_and_wait_waits_for_every_turn() {
        let manager = TurnManager::new();
        let a = manager.start("a", |cancellation| async move { cancellation.subscribe().changed().await.map_err(|_| RuntimeError::ChannelClosed)?; Ok(()) }).await.unwrap();
        let b = manager.start("b", |cancellation| async move { cancellation.subscribe().changed().await.map_err(|_| RuntimeError::ChannelClosed)?; Ok(()) }).await.unwrap();
        assert_eq!(manager.cancel_all_and_wait().await.unwrap(), 2);
        assert!(manager.state("a").await.is_none());
        assert!(manager.state("b").await.is_none());
        assert_eq!(a.state().await, TurnState::Cancelled);
        assert_eq!(b.state().await, TurnState::Cancelled);
    }
}
