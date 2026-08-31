use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::{AgentTurn, AgentTurnHandle, Cancellation, RuntimeError, TurnState};

const CANCEL_WAIT: Duration = Duration::from_secs(5);
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// Owns runtime turn concurrency, cancellation and active-turn handles.
/// Cross-process ownership is deliberately left to `Store`'s busy sentinel.
#[derive(Clone, Default)]
pub struct TurnManager {
    active: Arc<Mutex<HashMap<String, AgentTurnHandle>>>,
}

/// D-06 : garantit le retrait de l'entrée `active` même si le work panique —
/// le futur est alors détruit pendant l'unwind et le `Drop` s'exécute, ce qui
/// évite une session bloquée pour toujours en `TurnAlreadyActive`.
struct ActiveGuard {
    active: Arc<Mutex<HashMap<String, AgentTurnHandle>>>,
    key: String,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
    }
}

impl TurnManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sections critiques purement synchrones (get/insert/remove) : un mutex
    /// std suffit et reste utilisable depuis `Drop`.
    fn lock_active(&self) -> MutexGuard<'_, HashMap<String, AgentTurnHandle>> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
            let mut active = self.lock_active();
            if active.contains_key(&session_id) {
                return Err(RuntimeError::TurnAlreadyActive);
            }
            active.insert(session_id.clone(), handle.clone());
        }

        // D-06 : le garde est détenu par le work lui-même — retrait garanti à
        // la fin normale comme sur panic/unwind.
        let guard = ActiveGuard {
            active: self.active.clone(),
            key: session_id.clone(),
        };
        let start_result = turn
            .start(move |cancellation| async move {
                let _guard = guard;
                work(cancellation.clone()).await
            })
            .await;
        if start_result.is_err() {
            // La closure n'a jamais été appelée : le garde qu'elle capturait a
            // été détruit et a déjà retiré l'entrée. Sécurité défensive :
            self.lock_active().remove(&session_id);
        }
        start_result.map(|()| handle)
    }

    pub async fn cancel(&self, session_id: &str) -> Result<bool, RuntimeError> {
        let handle = self.lock_active().get(session_id).cloned();
        if let Some(handle) = handle {
            handle.cancel().await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Cancels a turn and waits until its worker has actually left the active set.
    pub async fn cancel_and_wait(&self, session_id: &str) -> Result<bool, RuntimeError> {
        if !self.cancel(session_id).await? {
            return Ok(false);
        }
        let deadline = Instant::now() + CANCEL_WAIT;
        while Instant::now() < deadline {
            if self.state(session_id).await.is_none() {
                return Ok(true);
            }
            tokio::time::sleep(CANCEL_POLL).await;
        }
        Err(RuntimeError::Task(format!(
            "timeout cancelling turn for session {session_id}"
        )))
    }

    pub async fn cancel_all(&self) -> Result<usize, RuntimeError> {
        let handles = self.lock_active().values().cloned().collect::<Vec<_>>();
        let mut cancelled = 0usize;
        for handle in handles {
            handle.cancel().await?;
            cancelled = cancelled.saturating_add(1);
        }
        Ok(cancelled)
    }

    /// Cancels every active turn and waits for all workers to leave the active set.
    ///
    /// C-17 : budget d'attente **partagé** — tous les tours sont annulés
    /// d'abord (les signaux partent en parallèle), puis la sortie est attendue
    /// avec un deadline global unique. Auparavant l'attente était séquentielle
    /// (CANCEL_WAIT par tour), ce qui dépassait le budget de shutdown global
    /// (SHUTDOWN_TIMEOUT) dès deux tours actifs.
    pub async fn cancel_all_and_wait(&self) -> Result<usize, RuntimeError> {
        let session_ids = self.lock_active().keys().cloned().collect::<Vec<_>>();
        let mut cancelled = 0usize;
        for session_id in &session_ids {
            if self.cancel(session_id).await? {
                cancelled = cancelled.saturating_add(1);
            }
        }
        let deadline = Instant::now() + CANCEL_WAIT;
        for session_id in session_ids {
            while Instant::now() < deadline && self.state(&session_id).await.is_some() {
                tokio::time::sleep(CANCEL_POLL).await;
            }
            if self.state(&session_id).await.is_some() {
                return Err(RuntimeError::Task(format!(
                    "timeout waiting for cancelled turn of session {session_id} (shared deadline)"
                )));
            }
        }
        Ok(cancelled)
    }

    pub async fn state(&self, session_id: &str) -> Option<TurnState> {
        let handle = self.lock_active().get(session_id).cloned();
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
        while !rx.borrow().is_terminal() {
            rx.changed().await.unwrap();
        }
    }

    async fn wait_for_cancellation(cancellation: Cancellation) -> Result<(), RuntimeError> {
        cancellation.cancelled().await;
        Ok(())
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
                tokio::select! {
                    _ = b1.notified() => Ok(()),
                    _ = rx.changed() => Ok(()),
                }
            })
            .await
        });
        let second = tokio::spawn(async move {
            m2.start("session", move |cancellation| async move {
                s2.fetch_add(1, Ordering::SeqCst);
                let mut rx = cancellation.subscribe();
                tokio::select! {
                    _ = b2.notified() => Ok(()),
                    _ = rx.changed() => Ok(()),
                }
            })
            .await
        });
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        let successes = [first.as_ref(), second.as_ref()]
            .into_iter()
            .filter(|result| result.is_ok())
            .count();
        let failures = [first.as_ref(), second.as_ref()]
            .into_iter()
            .filter(|result| matches!(result, Err(RuntimeError::TurnAlreadyActive)))
            .count();
        assert_eq!(successes, 1);
        assert_eq!(failures, 1);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        if let Ok(handle) = first {
            handle.cancel().await.unwrap();
            wait_for_terminal(&handle).await;
        }
        if let Ok(handle) = second {
            handle.cancel().await.unwrap();
            wait_for_terminal(&handle).await;
        }
        barrier.notify_waiters();
    }

    #[tokio::test]
    async fn cancel_and_wait_removes_turn_before_return() {
        let manager = TurnManager::new();
        let handle = manager
            .start("session", wait_for_cancellation)
            .await
            .unwrap();

        assert!(manager.cancel_and_wait("session").await.unwrap());
        assert!(manager.state("session").await.is_none());
        assert_eq!(handle.state().await, TurnState::Cancelled);
    }

    #[tokio::test]
    async fn cancel_all_and_wait_waits_for_every_turn() {
        let manager = TurnManager::new();
        let a = manager.start("a", wait_for_cancellation).await.unwrap();
        let b = manager.start("b", wait_for_cancellation).await.unwrap();

        assert_eq!(manager.cancel_all_and_wait().await.unwrap(), 2);
        assert!(manager.state("a").await.is_none());
        assert!(manager.state("b").await.is_none());
        assert_eq!(a.state().await, TurnState::Cancelled);
        assert_eq!(b.state().await, TurnState::Cancelled);
    }

    #[tokio::test]
    async fn panicking_turn_releases_the_active_entry() {
        // D-06 : un panic dans le work ne doit pas laisser une entrée
        // « zombie » dans la carte active (session bloquée en
        // TurnAlreadyActive pour toujours).
        let manager = TurnManager::new();
        manager
            .start("session", |_cancellation| async {
                panic!("simulated turn panic");
            })
            .await
            .unwrap();

        // Laisse la tâche paniquée finir son unwind.
        for _ in 0..50 {
            if manager.state("session").await.is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(manager.state("session").await.is_none());

        // La session doit pouvoir démarrer un nouveau tour.
        let restart = manager
            .start("session", |_| async { Ok(()) })
            .await
            .expect("session must be reusable after a panicking turn");
        restart.cancel().await.ok();
    }
}
