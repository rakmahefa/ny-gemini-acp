//! Session persistence operations.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{Live, Session, Store};

impl Store {
    pub async fn open(dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(dir).await.with_context(|| format!("création du répertoire {}", dir.display()))?;
        Ok(Self { dir: dir.to_path_buf(), live: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())) })
    }

    pub(crate) fn path(&self, id: &str) -> PathBuf { self.dir.join(format!("{id}.json")) }

    pub(crate) async fn persist(&self, session: &Session) -> Result<()> {
        let raw = serde_json::to_vec_pretty(session)?;
        let tmp = self.dir.join(format!(".{}.tmp", session.id));
        tokio::fs::write(&tmp, raw).await?;
        tokio::fs::rename(&tmp, self.path(&session.id)).await?;
        Ok(())
    }

    pub async fn create(&self, cwd: PathBuf, additional_directories: Vec<PathBuf>, model: &str) -> Result<Session> {
        let id = format!("sess_{}", uuid::Uuid::new_v4().simple());
        let session = Session::new(id.clone(), cwd, additional_directories, model);
        let (cancel, _) = tokio::sync::watch::channel(false);
        self.persist(&session).await?;
        self.live.write().await.insert(id, Live { session: session.clone(), cancel, busy: false, generation: 0 });
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> Option<Session> {
        let live = self.live.read().await;
        match live.get(id) { Some(entry) => Some(entry.session.clone()), None => drop(live), }
        if let Some(session) = self.read(id).await { self.live.write().await.insert(id.to_string(), Live { session: session.clone(), cancel: tokio::sync::watch::channel(false).0, busy: false, generation: 0 }); return Some(session); }
        None
    }

    pub(crate) async fn read(&self, id: &str) -> Option<Session> {
        let raw = tokio::fs::read(self.path(id)).await.ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub async fn list(&self, cwd: Option<&Path>) -> Vec<Session> {
        let mut out = Vec::new();
        let mut entries = match tokio::fs::read_dir(&self.dir).await { Ok(v) => v, Err(_) => return out };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") { continue; }
            if let Ok(raw) = tokio::fs::read(&path).await { if let Ok(session) = serde_json::from_slice::<Session>(&raw) { if cwd.map(|c| session.cwd == c).unwrap_or(true) { out.push(session); } } }
        }
        out.sort_by(|a,b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub async fn delete(&self, id: &str) -> bool {
        let removed = tokio::fs::remove_file(self.path(id)).await.is_ok();
        self.live.write().await.remove(id);
        removed
    }
}
