//! Inter-process `busy` sentinel management.

use super::Store;

impl Store {
    pub(crate) fn busy_path(&self, id: &str) -> std::path::PathBuf {
        self.dir.join(format!("{id}.busy"))
    }

    pub(crate) async fn acquire_busy(&self, id: &str) -> anyhow::Result<()> {
        let path = self.busy_path(id);
        let content = format!(
            "pid={} ts={}\n",
            std::process::id(),
            crate::time::now_unix()
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
                    Ok(())
                } else {
                    anyhow::bail!("session {id} already busy")
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) async fn release_busy(&self, id: &str) {
        let _ = tokio::fs::remove_file(self.busy_path(id)).await;
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

    tokio::fs::metadata(format!("/proc/{pid}")).await.is_err()
}
