use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{AcpTurn, AcpTurnHandle, EncapsError, TurnState};

struct ActiveTurn {
    generation: u64,
    handle: AcpTurnHandle,
}

/// Serializes ACP turns independently for each session.
#[derive(Clone, Default)]
pub struct TurnManager {
    active: Arc<Mutex<HashMap<String, ActiveTurn>>>,
}

impl TurnManager {
    pub fn new() -> Self { Self::default() }

    pub async fn start<F, Fut>(&self, session_id: impl Into<String>, work: F) -> Result<AcpTurnHandle, EncapsError>
    where
        F: FnOnce(crate::Cancellation) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), EncapsError>> + Send + 'static,
    {
        let session_id = session_id.into();
        let mut work = Some(work);

        loop {
            let previous = {
                let active = self.active.lock().await;
                active.get(&session_id).map(|entry| (entry.generation, entry.handle.clone()))
            };

            if let Some((generation, previous)) = previous {
                if !previous.state().await.is_terminal() {
                    let mut state = previous.subscribe_state();
                    while !state.borrow().is_terminal() {
                        if state.changed().await.is_err() { break; }
                    }
                }
                let mut active = self.active.lock().await;
                if active.get(&session_id).is_some_and(|entry| entry.generation == generation) {
                    active.remove(&session_id);
                }
                continue;
            }

            let (turn, handle) = AcpTurn::new();
            let work = work.take().expect("turn work consumed exactly once");
            turn.start(work).await?;
            let mut active = self.active.lock().await;
            if active.contains_key(&session_id) {
                drop(active);
                let mut state = handle.subscribe_state();
                while !state.borrow().is_terminal() {
                    if state.changed().await.is_err() { break; }
                }
                continue;
            }
            let generation = active.values().map(|entry| entry.generation).max().unwrap_or(0).saturating_add(1);
            active.insert(session_id, ActiveTurn { generation, handle: handle.clone() });
            return Ok(handle);
        }
    }

    pub async fn cancel(&self, session_id: &str) -> Result<bool, EncapsError> {
        let handle = self.active.lock().await.get(session_id).map(|entry| entry.handle.clone());
        if let Some(handle) = handle {
            handle.cancel().await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn state(&self, session_id: &str) -> Option<TurnState> {
        let handle = self.active.lock().await.get(session_id).map(|entry| entry.handle.clone());
        match handle {
            Some(handle) => Some(handle.state().await),
            None => None,
        }
    }
}
