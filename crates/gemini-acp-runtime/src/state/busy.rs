//! Gestion du sentinel inter-processus `busy` : fichier `<id>.busy` cohabitant
//! avec `<id>.json`. Présence = un tour est en cours (soit dans ce processus,
//! soit dans un autre process `gemini-acp` / `gemini-acp-snapshot`).
//!
//! Utilisé pour éviter que `gemini-acp-snapshot restore` n'écrase une
//! session pendant qu'un agent est en plein tour (data corruption).

use super::Store;

impl Store {
    /// Chemin du sentinel `busy`.
    pub(crate) fn busy_path(&self, id: &str) -> std::path::PathBuf {
        self.dir.join(format!("{id}.busy"))
    }

    /// Crée atomiquement le sentinel `busy`.
    ///
    /// Un sentinel appartenant à un processus vivant est une vraie collision
    /// inter-processus et doit être refusé. Un sentinel orphelin peut être
    /// récupéré après un crash lorsque son PID n'existe plus.
    pub(crate) async fn acquire_busy(&self, id: &str) -> anyhow::Result<()> {
        let path = self.busy_path(id);
        let content = format!(
            "pid={} ts={}\n",
            std::process::id(),
            gemini_acp_config::core::time::now_unix()
        );

        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(_) => {
                tokio::fs::write(&path, &content).await?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if stale_busy_sentinel(&path).await {
                    let _ = tokio::fs::remove_file(&path).await;
                    let file = tokio::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .await?;
                    drop(file);
                    tokio::fs::write(&path, &content).await?;
                    return Ok(());
                }
                anyhow::bail!("session {id} already busy")
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Supprime le sentinel `busy`. Appelée à `end_turn` / `TurnGuard::drop`.
    pub async fn release_busy(&self, id: &str) {
        let _ = tokio::fs::remove_file(self.busy_path(id)).await;
    }

    /// Force la session à l'état inactif : libère le flag mémoire `busy` et
    /// le sentinel disque.
    pub async fn force_idle(&self, id: &str) {
        if let Some(entry) = self.live.write().await.get_mut(id) {
            entry.busy = false;
        }
        self.release_busy(id).await;
    }
}

async fn stale_busy_sentinel(path: &std::path::Path) -> bool {
    let Ok(raw) = tokio::fs::read_to_string(path).await else {
        return false;
    };
    let Some(pid) = raw
        .lines()
        .find_map(|line| line.strip_prefix("pid=")?.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return false;
    };

    // Linux is the target runtime. `/proc/<pid>` lets us distinguish a stale
    // sentinel left by a crashed process from a live owner without guessing.
    tokio::fs::metadata(format!("/proc/{pid}")).await.is_err()
}
