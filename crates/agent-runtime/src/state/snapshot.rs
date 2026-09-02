//! Gestion des snapshots de session : création, liste, prune.
//!
//! Chaque snapshot est un fichier `<id>.<n>.snap.json` dans le dépôt de
//! sessions, où `n` est le nombre de messages avant la fin du tour.
//! Les snapshots sont créés par `end_turn` **APRÈS** le commit canonique de la
//! session (un snapshot est un artefact de récupération auxiliaire qui ne doit
//! jamais être plus récent que l'état canonique pendant un échec d'écriture),
//! puis élagués pour ne garder que les `MAX_SNAPSHOTS` plus récents.

use anyhow::Result;
use tokio::fs;

use super::Store;

impl Store {
    /// Chemin d'un snapshot `<id>.<n>.snap.json`.
    pub(crate) fn snapshot_path(&self, id: &str, n: usize) -> std::path::PathBuf {
        self.dir.join(format!("{id}.{n}.snap.json"))
    }

    /// Écrit un snapshot via le même mécanisme atomique que les sessions.
    pub(super) async fn persist_snapshot(&self, id: &str, n: usize, raw: &[u8]) -> Result<()> {
        use anyhow::Context;
        Self::write_atomic(&self.snapshot_path(id, n), raw)
            .await
            .context("failed to write session snapshot")
    }

    /// Garde seulement les `keep` snapshots les plus récents (par n décroissant).
    pub(super) async fn prune_snapshots(&self, id: &str, keep: usize) {
        let snaps = self.list_snapshots(id).await;
        if snaps.len() <= keep {
            return;
        }
        for n in &snaps[keep..] {
            let _ = fs::remove_file(self.snapshot_path(id, *n)).await;
        }
    }

    /// Liste les numéros de snapshots disponibles pour une session (décroissant).
    pub async fn list_snapshots(&self, id: &str) -> Vec<usize> {
        let prefix = format!("{id}.");
        let suffix = ".snap.json";
        let mut snaps = Vec::new();
        let Ok(mut entries) = fs::read_dir(&self.dir).await else {
            return snaps;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stripped) = name
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(suffix))
            {
                if let Ok(n) = stripped.parse::<usize>() {
                    snaps.push(n);
                }
            }
        }
        snaps.sort_by(|a, b| b.cmp(a));
        snaps
    }
}
