//! ACP stdio adapter: configuration, provider construction and transport only.
use acp_adaptor::agent::run_agent;
use agent_runtime::{AgentRuntime, RuntimeConfig};
use anyhow::{Context, Result};
use llm_provider::{AgentConfig, GeminiProvider};
use std::sync::Arc;
use tools_provider::DefaultToolProvider;
#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = AgentConfig::from_env();
    let llm = Arc::new(GeminiProvider::from_agent_config(&config).await?);
    let tools = Arc::new(DefaultToolProvider::from_env().await?);
    let runtime = AgentRuntime::new(
        RuntimeConfig {
            data_dir: config.data_dir.clone(),
            default_model: config.default_model.clone(),
        },
        llm,
        tools,
    )
    .await?;
    tokio::select! {result=run_agent(runtime.state().clone())=>{result.context("ACP transport stopped with an error")?;}_=wait_for_shutdown_signal()=>{runtime.shutdown().await;tracing::info!("graceful shutdown completed")}}
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
            Ok(s) => s,
            Err(error) => {
                tracing::error!(%error,"failed to install SIGTERM handler");
                return;
            }
        };
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(error) => {
                tracing::error!(%error,"failed to install SIGINT handler");
                return;
            }
        };
        tokio::select! {_=sigterm.recv()=>tracing::info!("SIGTERM received"),_=sigint.recv()=>tracing::info!("SIGINT received")}
    }
    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error,"failed to install Ctrl-C handler")
        } else {
            tracing::info!("Ctrl-C received")
        }
    }
}
