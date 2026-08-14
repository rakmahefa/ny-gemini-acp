use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{AcpTurn, AcpTurnHandle, Cancellation, EncapsError, TurnState};

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

    pub async fn start<F, Fut>(&self, session_id: impl Into<String>, work: F) -> Result<AcpTurnHandle, EncapsError>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let session_id = session_id.into();
        let lock = self.session_lock(&session_id).await;
        let (turn, handle) = AcpTurn::new();
        self.active.lock().await.insert(session_id.clone(), handle.clone());

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

        if start_result.is_err() {
            self.active.lock().await.remove(&session_id);
        }
        start_result.map(|()| handle)
    }

    pub async fn cancel(&self, session_id: &str) -> Result<bool, EncapsError> {
        let handle = self.active.lock().await.get(session_id).cloned();
        if let Some(handle) = handle {
            handle.cancel().await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn state(&self, session_id: &str) -> Option<TurnState> {
        let handle = self.active.lock().await.get(session_id).cloned();
        match handle {
            Some(handle) => Some(handle.state().await),
            None => None,
        }
    }
}
