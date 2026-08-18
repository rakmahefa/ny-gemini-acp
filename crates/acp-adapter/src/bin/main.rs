//! Binaire `gemini-acp` : bootstrap Gemini + runtime + transport ACP.

use std::sync::Arc;

use anyhow::{Context, Result};

use acp_adaptor::run_agent;
use gemini_acp_config::{client::Client, AgentConfig};
use gemini_acp_runtime::AgentRuntime;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = AgentConfig::from_env();
    let provider = Arc::new(
        Client::new(gemini_acp_config::client::Config {
            cookie_file: config.cookie_file.clone(),
            default_model: config.default_model.clone(),
            auth_user: config.auth_user,
            proxy: config.proxy.clone(),
            ..Default::default()
        })
        .await
        .context("initialisation du provider Gemini")?,
    );
    let runtime = AgentRuntime::from_parts(config, provider).await?;

    tokio::select! {
        result = run_agent(runtime.state().clone()) => {
            result.context("transport ACP arrêté avec une erreur")?;
        }
        _ = wait_for_shutdown_signal() => {
            runtime.shutdown().await;
            tracing::info!("shutdown gracieux terminé");
        }
    }
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => { tracing::error!(%error, "installation du handler SIGTERM impossible"); return; }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(error) => { tracing::error!(%error, "installation du handler SIGINT impossible"); return; }
        };
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM reçu"),
            _ = sigint.recv() => tracing::info!("SIGINT reçu"),
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await { tracing::error!(%error, "installation du handler Ctrl-C impossible"); }
        else { tracing::info!("Ctrl-C reçu"); }
    }
}
