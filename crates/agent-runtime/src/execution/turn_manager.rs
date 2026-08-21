use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{AgentTurn, AgentTurnHandle, Cancellation, RuntimeError, TurnState};

/// Owns runtime turn concurrency, cancellation and active-turn handles.
/// Cross-process ownership is deliberately left to `Store`'s busy sentinel.
#[derive(Clone, Default)]
pub struct TurnManager {
    active: Arc<Mutex<HashMap<String, AgentTurnHandle>>>,
}

impl TurnManager {
    pub fn new() -> Self { Self::default() }

    /// Starts one turn for `session_id`.
    ///
    /// The reservation is installed before the worker is spawned, so concurrent starts
    /// deterministically produce exactly one winner.
    pub async fn start<F, Fut>(
        &self,
        session_id: impl Into<String>,
        work: F,
    ) -> Result<AgentTurnHandle, RuntimeError>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), RuntimeError>> + Send + 'static,
    {
        let session_id = session_id.into();
        let (turn, handle) = AgentTurn::new();
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&session_id) {
                return Err(RuntimeError::TurnAlreadyActive);
            }
            active.insert(session_id.clone(), handle.clone());
        }

        let active = self.active.clone();
        let key = session_id.clone();
        let start_result = turn
            .start(move |cancellation| async move {
                let result = work(cancellation.clone()).await;
                active.lock().await.remove(&key);
                result
            })
            .await;

        if start_result.is_err() {
            self.active.lock().await.remove(&session_id);
        }
        start_result.map(|()| handle)
    }

    /// Requests cancellation of the currently active turn, if any.
    pub async fn cancel(&self, session_id: &str) -> Result<bool, RuntimeError> {
        let handle = self.active.lock().await.get(session_id).cloned();
        if let Some(handle) = handle {
            handle.cancel().await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Requests cancellation of every active turn and returns how many were signalled.
    pub async fn cancel_all(&self) -> Result<usize, RuntimeError> {
        let handles = self.active.lock().await.values().cloned().collect::<Vec<_>>();
        let mut cancelled = 0usize;
        for handle in handles {
            handle.cancel().await?;
            cancelled = cancelled.saturating_add(1);
        }
        Ok(cancelled)
    }

    pub async fn state(&self, session_id: &str) -> Option<TurnState> {
        let handle = self.active.lock().await.get(session_id).cloned();
        match handle {
            Some(handle) => Some(handle.state().await),
            None => None,
        }
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
        let m1 = manager.clone();
        let m2 = manager.clone();
        let b1 = barrier.clone();
        let b2 = barrier.clone();
        let s1 = starts.clone();
        let s2 = starts.clone();
        let first = tokio::spawn(async move {
            m1.start("session", move |cancellation| async move {
                s1.fetch_add(1, Ordering::SeqCst);
                let mut rx = cancellation.subscribe();
                tokio::select! { _ = b1.notified() => Ok(()), _ = rx.changed() => Ok(()) }
            }).await
        });
        let second = tokio::spawn(async move {
            m2.start("session", move |cancellation| async move {
                s2.fetch_add(1, Ordering::SeqCst);
                let mut rx = cancellation.subscribe();
                tokio::select! { _ = b2.notified() => Ok(()), _ = rx.changed() => Ok(()) }
            }).await
        });
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        let successes = [first.as_ref(), second.as_ref()].into_iter().filter(|result| result.is_ok()).count();
        let failures = [first.as_ref(), second.as_ref()].into_iter().filter(|result| matches!(result, Err(RuntimeError::TurnAlreadyActive))).count();
        assert_eq!(successes, 1);
        assert_eq!(failures, 1);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        if let Ok(handle) = first { handle.cancel().await.unwrap(); wait_for_terminal(&handle).await; }
        if let Ok(handle) = second { handle.cancel().await.unwrap(); wait_for_terminal(&handle).await; }
        barrier.notify_waiters();
    }

    #[tokio::test]
    async fn reservation_is_released_after_completion() {
        let manager = TurnManager::new();
        let first = manager.start("session", |_cancellation| async { Ok(()) }).await.unwrap();
        wait_for_terminal(&first).await;
        assert!(manager.state("session").await.is_none());
        let second = manager.start("session", |_cancellation| async { Ok(()) }).await.unwrap();
        wait_for_terminal(&second).await;
    }

    #[tokio::test]
    async fn cancel_all_signals_every_active_turn() {
        let manager = TurnManager::new();
        let a = manager.start("a", |cancellation| async move {
            cancellation.subscribe().changed().await.map_err(|_| RuntimeError::ChannelClosed)?;
            Ok(())
        }).await.unwrap();
        let b = manager.start("b", |cancellation| async move {
            cancellation.subscribe().changed().await.map_err(|_| RuntimeError::ChannelClosed)?;
            Ok(())
        }).await.unwrap();
        assert_eq!(manager.cancel_all().await.unwrap(), 2);
        wait_for_terminal(&a).await;
        wait_for_terminal(&b).await;
        assert_eq!(a.state().await, TurnState::Cancelled);
        assert_eq!(b.state().await, TurnState::Cancelled);
    }
}
