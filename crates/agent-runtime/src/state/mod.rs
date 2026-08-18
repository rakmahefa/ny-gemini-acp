use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::{watch, RwLock};

use crate::tools::lifecycle::{
    begin_partial_output, bind_session_cancellation, take_partial_output,
    unbind_session_cancellation,
};
use gemini_acp_encaps::Cancellation;

mod busy;
mod persistence;
mod snapshot;
mod types;

pub(crate) use types::MAX_SNAPSHOTS;
pub use types::{Live, Role, Session, SessionMode, TurnError};

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    pub(crate) live: Arc<RwLock<HashMap<String, Live>>>,
}

impl Store {
    pub async fn begin_turn(
        &self,
        id: &str,
    ) -> Result<(Session, watch::Receiver<bool>, u64), TurnError> {
        let mut live = self.live.write().await;
        if let Some(entry) = live.get_mut(id) {
            if entry.busy {
                return Err(TurnError::AlreadyRunning);
            }
            self.acquire_busy(id)
                .await
                .map_err(|_| TurnError::AlreadyRunning)?;
            entry.busy = true;
            entry.generation += 1;
            let gen = entry.generation;
            entry.cancel = Cancellation::new();
            let rx = entry.cancel.subscribe();
            bind_session_cancellation(id, entry.cancel.clone());
            begin_partial_output(id);
            return Ok((entry.session.clone(), rx, gen));
        }
        let session = self
            .read(id)
            .await
            .ok_or_else(|| TurnError::NotFound(id.to_string()))?;
        self.acquire_busy(id)
            .await
            .map_err(|_| TurnError::AlreadyRunning)?;
        let cancellation = Cancellation::new();
        let rx = cancellation.subscribe();
        bind_session_cancellation(id, cancellation.clone());
        begin_partial_output(id);
        live.insert(
            id.to_string(),
            Live {
                session: session.clone(),
                cancel: cancellation,
                busy: true,
                generation: 1,
            },
        );
        Ok((session, rx, 1))
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

        // A cancelled stream may have already emitted assistant chunks while
        // never reaching the normal `total_output` finalization in prompt/turn.
        // Persist that partial text only when the current session history still
        // ends at the user message: this proves the streamed answer has not
        // already been committed by the normal completion path.
        let partial = take_partial_output(id);
        if !partial.trim().is_empty() && matches!(session.messages.last(), Some((Role::User, _))) {
            session.messages.push((Role::Assistant, partial));
        }

        session.updated_at = gemini_acp_config::core::time::now_iso();
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
            entry.busy = false;
        }
        unbind_session_cancellation(id);
        self.release_busy(id).await;
        persist_result
    }

    pub async fn cancel(&self, id: &str) {
        let live = self.live.read().await;
        if let Some(entry) = live.get(id) {
            entry.cancel.cancel();
        }
    }

    pub async fn cancel_all(&self) {
        let live = self.live.read().await;
        for (id, entry) in live.iter() {
            entry.cancel.cancel();
            tracing::debug!(session = %id, "session cancellation requested");
        }
    }

    pub async fn close(&self, id: &str) -> bool {
        let mut live = self.live.write().await;
        let existed = live.contains_key(id) || self.path(id).exists();
        if let Some(entry) = live.get(id) {
            entry.cancel.cancel();
        }
        live.remove(id);
        drop(live);
        let _ = take_partial_output(id);
        unbind_session_cancellation(id);
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
                cancel: Cancellation::new(),
                busy: false,
                generation: 0,
            },
        );
        Ok(forked)
    }
}

#[cfg(test)]
#[path = "../test/state.rs"]
mod tests;
