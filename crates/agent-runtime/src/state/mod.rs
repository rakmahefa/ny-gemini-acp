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
        // The filesystem sentinel is the inter-process ownership boundary. Do
        // not hold the global in-memory write lock across filesystem I/O.
        self.acquire_busy(id)
            .await
            .map_err(|_| TurnError::AlreadyRunning)?;

        let mut live = self.live.write().await;
        if let Some(entry) = live.get_mut(id) {
            entry.generation = entry.generation.saturating_add(1);
            return Ok((entry.session.clone(), entry.generation));
        }

        let session = match self.read(id).await {
            Some(session) => session,
            None => {
                drop(live);
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
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;
        f(&mut session);
        self.persist(&session).await?;
        Ok(())
    }

    pub async fn end_turn(&self, id: &str, session: Session, expected_gen: u64) -> Result<(), StoreError> {
        // D-05 : le verrou d'écriture est tenu jusqu'au commit. `end_turn` est
        // appelé une seule fois par tour : la contention acceptable achète une
        // atomité réelle face à un `update_session` concurrent (fusion) et face
        // à un `delete` concurrent (plus de résurrection de session supprimée).
        let mut live = self.live.write().await;
        let Some(entry) = live.get_mut(id) else {
            // La session a été supprimée pendant le tour (`Store::delete` retire
            // l'entrée live) : on abandonne le commit au lieu de réécrire le JSON
            // et de « ressusciter » la session supprimée.
            drop(live);
            self.release_busy(id).await;
            tracing::warn!(session = %id, "end_turn: session deleted during turn, commit aborted");
            return Err(StoreError::SessionDeleted(id.to_string()));
        };
        if expected_gen != 0 && entry.generation != expected_gen {
            let current = entry.generation;
            drop(live);
            self.release_busy(id).await;
            tracing::warn!(session = %id, expected_gen, current, "end_turn: stale turn ignored");
            return Err(StoreError::StaleGeneration {
                expected: expected_gen,
                current,
            });
        }

        let mut final_session = session;
        // D-05 : fusion au lieu d'écrasement — les champs de configuration
        // modifiables en concurrent pendant le tour (session/set_mode,
        // set_model, set_think, tools_enabled, titre, répertoires) font foi
        // depuis l'entrée live ; le tour ne contribue que l'historique et
        // turn_count.
        let live_session = &entry.session;
        final_session.cwd = live_session.cwd.clone();
        final_session.additional_directories = live_session.additional_directories.clone();
        final_session.title = live_session.title.clone();
        final_session.model = live_session.model.clone();
        final_session.think = live_session.think;
        final_session.tools_enabled = live_session.tools_enabled;
        final_session.mode = live_session.mode;

        final_session.messages.normalize_legacy();
        final_session.updated_at = crate::time::now_iso();
        final_session.turn_count = final_session.turn_count.saturating_add(1);

        // Commit the canonical session first. A snapshot is an auxiliary
        // recovery artifact and must never become newer than the canonical
        // state while the canonical write is failing.
        let persist_result = self.persist(&final_session).await.map_err(|error| {
            StoreError::Persistence(error.to_string())
        });

        if persist_result.is_ok() {
            if !final_session.messages.is_empty() {
                let snap_n = final_session.messages.len();
                match serde_json::to_vec_pretty(&final_session) {
                    Ok(raw) => {
                        if let Err(error) = self.persist_snapshot(id, snap_n, &raw).await {
                            tracing::warn!(session = %id, snapshot = snap_n, error = %error, "snapshot persist failed after canonical session commit");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(session = %id, snapshot = snap_n, error = %error, "snapshot serialization failed after canonical session commit");
                    }
                }
                self.prune_snapshots(id, MAX_SNAPSHOTS).await;
            }
            if let Some(entry) = live.get_mut(id) {
                entry.session = final_session;
            }
        }

        drop(live);
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
            .ok_or_else(|| anyhow::anyhow!("source session not found: {source_id}"))?;
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
