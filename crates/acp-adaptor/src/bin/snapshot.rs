//! Utilitaire CLI pour gérer les snapshots de sessions ACP.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let cmd = args
        .next()
        .context("usage: gemini-acp-snapshot <list|restore|sessions> [args]")?;

    let data_dir = resolve_data_dir();
    let store = gemini_acp_runtime::state::Store::open(&data_dir)
        .await
        .with_context(|| format!("ouverture dépôt {}", data_dir.display()))?;

    match cmd.as_str() {
        "list" => {
            let session_id = args
                .next()
                .context("usage: gemini-acp-snapshot list <session_id>")?;
            let snaps = store.list_snapshots(&session_id).await;
            if snaps.is_empty() {
                println!("Aucun snapshot pour la session {session_id}.");
            } else {
                println!("Snapshots pour la session {session_id} (n° de tour, décroissant) :");
                for n in &snaps {
                    println!("  turn={n}");
                }
            }
        }
        "restore" => {
            let mut session_id = None;
            let mut turn_str = None;
            let mut force = false;
            for arg in args {
                if arg == "--force" {
                    force = true;
                } else if session_id.is_none() {
                    session_id = Some(arg);
                } else if turn_str.is_none() {
                    turn_str = Some(arg);
                } else {
                    bail!("argument inattendu: {arg}");
                }
            }
            let session_id = session_id
                .context("usage: gemini-acp-snapshot restore <session_id> <turn> [--force]")?;
            let turn_str = turn_str.context("turn manquant")?;
            let turn: usize = turn_str
                .parse()
                .with_context(|| format!("turn invalide: {turn_str} (entier attendu)"))?;
            let result = if force {
                store.restore_snapshot_force(&session_id, turn).await
            } else {
                store.restore_snapshot(&session_id, turn).await
            };
            result.with_context(|| format!("restauration snapshot {turn} de {session_id}"))?;
            println!("✓ Session {session_id} restaurée au tour {turn} (état après ce tour).");
        }
        "sessions" => {
            let sessions = store.list(None).await;
            if sessions.is_empty() {
                println!("Aucune session dans le dépôt {}.", data_dir.display());
            } else {
                println!("Sessions dans le dépôt {} :", data_dir.display());
                for s in &sessions {
                    let snaps = store.list_snapshots(&s.id).await;
                    let title = s.title.as_deref().unwrap_or("(sans titre)");
                    println!(
                        "  {} — {} — {} messages, {} snapshot(s)",
                        s.id,
                        title,
                        s.messages.len(),
                        snaps.len()
                    );
                }
            }
        }
        other => {
            bail!("sous-commande inconnue: {other} (attendu: list|restore|sessions)");
        }
    }

    Ok(())
}

fn resolve_data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GEMINI_ACP_DATA_DIR") {
        return PathBuf::from(d);
    }
    if let Ok(x) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(x).join("gemini-acp");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/gemini-acp");
    }
    PathBuf::from(".")
}
