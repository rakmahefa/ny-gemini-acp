use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

mod busy;
pub mod history;
mod persistence;
mod snapshot;
mod types;

pub use history::{History, HistoryEntry};
pub(crate) use types::MAX_SNAPSHOTS;
pub use types::{Live, Role, Session, SessionMode, StoreError, TurnError};

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    pub(crate) live: Arc<RwLock<HashMap<String, Live>>>,
}

impl Store {
    pub async fn begin_turn(&self, id: &str) -> Result<(Session, u64), TurnError> {
        let mut live = self.live.write().await;
        self.acquire_busy(id)
            .await
            .map_err(|_| TurnError::AlreadyRunning)?;

        if let Some(entry) = live.get_mut(id) {
            entry.generation = entry.generation.saturating_add(1);
            return Ok((entry.session.clone(), entry.generation));
        }

        let session = match self.read(id).await {
            Some(session) => session,
            None => {
                self.release_busy(id).await;
                return Err(TurnError::NotFound(id.to_string()));
            }
        };
        live.insert(
            id.to_string(),
            Live {
                session: session.clone(),
                generation: 1,
            },
        );
        Ok((session, 1))
    }

    pub async fn update_session<F>(&self, id: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut Session),
    {
        let mut live = self.live.write().await;
        if let Some(entry) = live.get_mut(id) {
            let mut updated = entry.session.clone();
            f(&mut updated);
            self.persist(&updated).await?;
            entry.session = updated;
            return Ok(());
        }

        let mut session = self
            .read(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("session introuvable: {id}"))?;
        f(&mut session);
        self.persist(&session).await?;
        Ok(())
    }

    pub async fn end_turn(&self, id: &str, session: Session, expected_gen: u64) -> Result<(), StoreError> {
        let current_gen = {
            let live = self.live.read().await;
            live.get(id).map(|entry| entry.generation)
        };
        if expected_gen != 0 {
            if let Some(current_gen) = current_gen {
                if current_gen != expected_gen {
                    tracing::warn!(session = %id, expected_gen, current_gen, "end_turn: tour obsolète ignoré");
                    return Err(StoreError::StaleGeneration {
                        expected: expected_gen,
                        current: current_gen,
                    });
                }
            }
        }

        let mut final_session = session;
        final_session.messages.normalize_legacy();
        final_session.updated_at = crate::time::now_iso();
        final_session.turn_count = final_session.turn_count.saturating_add(1);

        if !final_session.messages.is_empty() {
            let snap_n = final_session.messages.len();
            if let Ok(raw) = serde_json::to_vec_pretty(&final_session) {
                if let Err(error) = self.persist_snapshot(id, snap_n, &raw).await {
                    tracing::error!(session = %id, snapshot = snap_n, error = %error, "snapshot persist failed");
                }
            }
            self.prune_snapshots(id, MAX_SNAPSHOTS).await;
        }

        let persist_result = self.persist(&final_session).await.map_err(|error| {
            StoreError::Persistence(error.to_string())
        });
        if persist_result.is_ok() {
            if let Some(entry) = self.live.write().await.get_mut(id) {
                entry.session = final_session;
            }
        }
        self.release_busy(id).await;
        persist_result
    }

    pub async fn close(&self, id: &str) -> bool {
        let mut live = self.live.write().await;
        let existed = live.contains_key(id) || self.path(id).exists();
        live.remove(id);
        drop(live);
        self.release_busy(id).await;
        existed
    }

    pub async fn fork(&self, source_id: &str) -> Result<Session> {
        let source = self
            .get(source_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("session source introuvable: {source_id}"))?;
        let new_id = format!("sess_{}", uuid::Uuid::new_v4().simple());
        let forked = source.fork(new_id);
        self.persist(&forked).await?;
        self.live.write().await.insert(
            forked.id.clone(),
            Live {
                session: forked.clone(),
                generation: 0,
            },
        );
        Ok(forked)
    }
}

#[cfg(test)]
#[path = "../test/state.rs"]
mod tests;
