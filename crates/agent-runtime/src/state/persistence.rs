//! Session persistence operations.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{Live, Session, Store};

impl Store {
    pub async fn open(dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("création du répertoire {}", dir.display()))?;
        cleanup_orphan_tmp_files(dir).await;
        let sessions_dir = dir.join("sessions");
        if tokio::fs::metadata(&sessions_dir).await.is_ok() {
            cleanup_orphan_tmp_files(&sessions_dir).await;
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            live: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        })
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
        self.persist(&session).await?;
        self.live.write().await.insert(id, Live { session: session.clone(), generation: 0 });
        Ok(session)
    }

    pub async fn get(&self, id: &str) -> Option<Session> {
        {
            let live = self.live.read().await;
            if let Some(entry) = live.get(id) { return Some(entry.session.clone()); }
        }

        if let Some(session) = self.read(id).await {
            self.live.write().await.insert(id.to_string(), Live { session: session.clone(), generation: 0 });
            return Some(session);
        }
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
            if let Ok(raw) = tokio::fs::read(&path).await {
                if let Ok(session) = serde_json::from_slice::<Session>(&raw) {
                    if cwd.map(|c| session.cwd == c).unwrap_or(true) { out.push(session); }
                }
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub async fn delete(&self, id: &str) -> bool {
        let existed = self.live.write().await.remove(id).is_some() || self.path(id).exists();
        let _ = tokio::fs::remove_file(self.path(id)).await;
        existed
    }
}

async fn cleanup_orphan_tmp_files(dir: &Path) {
    let mut entries = match tokio::fs::read_dir(dir).await { Ok(entries) => entries, Err(_) => return };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_tmp = path.is_file()
            && path.file_name().and_then(|name| name.to_str())
                .map(|name| name.ends_with(".json.tmp") || name.ends_with(".tmp"))
                .unwrap_or(false);
        if is_tmp { let _ = tokio::fs::remove_file(path).await; }
    }
}
