use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::RwLock;

mod busy;
pub mod history;
mod persistence;
mod snapshot;
mod types;

pub use history::{History, HistoryEntry};
pub(crate) use types::MAX_SNAPSHOTS;
pub use types::{Live, Role, Session, SessionMode, TurnError};

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    pub(crate) live: Arc<RwLock<HashMap<String, Live>>>,
}

impl Store {
    /// Begins a persistence transaction for a turn.
    ///
    /// Runtime concurrency and cancellation are owned by `TurnManager`; this method only
    /// acquires the cross-process busy sentinel and advances a persistence generation.
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
            f(&mut entry.session);
            self.persist(&entry.session).await?;
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

    pub async fn end_turn(&self, id: &str, mut session: Session, expected_gen: u64) -> Result<()> {
        if expected_gen != 0 {
            let live = self.live.read().await;
            if let Some(entry) = live.get(id) {
                if entry.generation != expected_gen {
                    tracing::warn!(
                        session = %id,
                        expected_gen,
                        current_gen = entry.generation,
                        "end_turn: tour obsolète ignoré"
                    );
                    bail!(
                        "tour obsolète: génération attendue {expected_gen}, courante {}",
                        entry.generation
                    );
                }
            }
        }

        session.messages.normalize_legacy();
        session.updated_at = crate::time::now_iso();
        session.turn_count += 1;
        if let Some(current) = self.get(id).await {
            if !current.messages.is_empty() {
                let snap_n = current.messages.len();
                if let Ok(raw) = serde_json::to_string_pretty(&current) {
                    let _ = tokio::fs::write(self.snapshot_path(id, snap_n), &raw).await;
                }
                self.prune_snapshots(id, MAX_SNAPSHOTS).await;
            }
        }
        let persist_result = self.persist(&session).await;
        if let Some(entry) = self.live.write().await.get_mut(id) {
            entry.session = session.clone();
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
