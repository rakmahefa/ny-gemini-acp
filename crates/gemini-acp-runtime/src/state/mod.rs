//! Persistent and live session state.

mod busy;
mod persistence;
mod snapshot;
mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::{Mutex, RwLock};

pub(crate) use types::MAX_SNAPSHOTS;
pub use types::{Live, Role, Session, SessionMode, TurnError};

pub struct Store {
    dir: PathBuf,
    pub(crate) live: Arc<RwLock<HashMap<String, Live>>>,
    pub(crate) locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Store {
    pub async fn update_session<F>(&self, id: &str, f: F) -> Result<()> where F: FnOnce(&mut Session) {
        let mut live = self.live.write().await;
        if let Some(entry) = live.get_mut(id) { f(&mut entry.session); self.persist(&entry.session).await?; return Ok(()); }
        let mut session = self.read(id).await.ok_or_else(|| anyhow::anyhow!("session introuvable: {id}"))?;
        f(&mut session); self.persist(&session).await?; Ok(())
    }

    pub async fn end_turn(&self, id: &str, mut session: Session, expected_gen: u64) -> Result<()> {
        if expected_gen != 0 {
            let live = self.live.read().await;
            if let Some(entry) = live.get(id) {
                if entry.generation != expected_gen {
                    tracing::warn!(session = %id, expected_gen, current_gen = entry.generation, "end_turn: tour obsolète ignoré");
                    bail!("tour obsolète: génération attendue {expected_gen}, courante {}", entry.generation);
                }
            }
        }
        session.updated_at = gemini_acp_config::core::time::now_iso();
        session.turn_count += 1;
        if let Some(current) = self.get(id).await {
            if !current.messages.is_empty() { session.messages = current.messages; }
        }
        self.persist(&session).await?;
        self.live.write().await.insert(id.to_string(), Live { session, cancel: tokio::sync::watch::channel(false).0, busy: false, generation: expected_gen });
        Ok(())
    }
}

#[cfg(test)]
mod test;
