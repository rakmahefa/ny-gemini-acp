use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{AcpTurn, AcpTurnHandle, Cancellation, EncapsError, TurnState};

/// Coordinates turns so a session has at most one active turn at a time.
///
/// The manager owns the reservation, while `AcpTurn` owns the execution
/// lifecycle. A reservation is installed before the worker is spawned, making
/// concurrent `start` calls deterministic: exactly one wins the session slot.
#[derive(Clone, Default)]
pub struct TurnManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    active: Arc<Mutex<HashMap<String, AcpTurnHandle>>>,
}

impl TurnManager {
    pub fn new() -> Self { Self::default() }

    async fn session_lock(&self, session_id: &str) -> Arc<Mutex<()>> {
        let mut sessions = self.sessions.lock().await;
        sessions.entry(session_id.to_owned()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    /// Starts one turn for `session_id`.
    ///
    /// At most one turn can be active for a session. A competing call fails
    /// before spawning work, so it cannot replace or orphan the first handle.
    /// The returned handle remains valid after the manager removes its active
    /// reservation when the turn reaches a terminal state.
    pub async fn start<F, Fut>(&self, session_id: impl Into<String>, work: F) -> Result<AcpTurnHandle, EncapsError>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let session_id = session_id.into();
        let lock = self.session_lock(&session_id).await;
        let (turn, handle) = AcpTurn::new();
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&session_id) { return Err(EncapsError::TurnAlreadyActive); }
            active.insert(session_id.clone(), handle.clone());
        }
        let active = self.active.clone();
        let key = session_id.clone();
        let start_result = turn.start(move |cancellation| async move {
            let mut cancellation_rx = cancellation.subscribe();
            let guard = tokio::select! {
                guard = lock.lock() => guard,
                _ = cancellation_rx.changed() => return Ok(()),
            };
            if cancellation.is_cancelled() {
                drop(guard);
                active.lock().await.remove(&key);
                return Ok(());
            }
            let result = work(cancellation.clone()).await;
            drop(guard);
            active.lock().await.remove(&key);
            result
        }).await;
        if start_result.is_err() { self.active.lock().await.remove(&session_id); }
        start_result.map(|()| handle)
    }

    /// Requests cancellation of the currently active turn, if any.
    ///
    /// Returning `false` means the session has no active turn; this is not an
    /// error and makes cancellation safe to race with normal completion.
    pub async fn cancel(&self, session_id: &str) -> Result<bool, EncapsError> {
        let handle = self.active.lock().await.get(session_id).cloned();
        if let Some(handle) = handle { handle.cancel().await?; return Ok(true); }
        Ok(false)
    }

    /// Returns the state of the currently reserved turn, if one exists.
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

    async fn wait_for_terminal(handle: &AcpTurnHandle) {
        let mut rx = handle.subscribe_state();
        while !rx.borrow().is_terminal() { rx.changed().await.unwrap(); }
    }

    #[tokio::test]
    async fn concurrent_starts_have_exactly_one_winner() {
        let manager = TurnManager::new();
        let barrier = Arc::new(Notify::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let m1 = manager.clone(); let m2 = manager.clone();
        let b1 = barrier.clone(); let b2 = barrier.clone();
        let s1 = starts.clone(); let s2 = starts.clone();
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
        let failures = [first.as_ref(), second.as_ref()].into_iter().filter(|result| matches!(result, Err(EncapsError::TurnAlreadyActive))).count();
        assert_eq!(successes, 1);
        assert_eq!(failures, 1);
        // The single winning start owns the worker, so its work closure is
        // expected to begin exactly once before `start` returns.
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
}
