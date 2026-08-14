use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{AcpTurn, AcpTurnHandle, EncapsError, TurnState};

/// Serializes ACP turns independently for each session.
///
/// A new turn waits for the previous turn of the same session to reach a
/// terminal state. Turns belonging to different sessions are independent.
#[derive(Clone, Default)]
pub struct TurnManager {
    active: Arc<Mutex<HashMap<String, AcpTurnHandle>>>,
}

impl TurnManager {
    pub fn new() -> Self {
        Self::default()
    }

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
                active.get(&session_id).cloned()
            };

            if let Some(previous) = previous {
                if !previous.state().await.is_terminal() {
                    let mut state = previous.subscribe_state();
                    while !state.borrow().is_terminal() {
                        if state.changed().await.is_err() {
                            break;
                        }
                    }
                }

                let mut active = self.active.lock().await;
                if active
                    .get(&session_id)
                    .is_some_and(|current| current.state().now_or_never().flatten().is_some())
                {
                    // State was checked above; remove the completed generation.
                    active.remove(&session_id);
                }
                continue;
            }

            let (turn, handle) = AcpTurn::new();
            let work = work.take().expect("turn work consumed exactly once");
            turn.start(work).await?;
            self.active.lock().await.insert(session_id.clone(), handle.clone());
            return Ok(handle);
        }
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
