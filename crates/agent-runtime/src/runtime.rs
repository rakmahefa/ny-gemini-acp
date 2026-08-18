//! Provider-neutral runtime composition root.
use crate::events::EventBus;
use crate::session::SessionManager;
use crate::{SharedLlmProvider, SharedToolProvider};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub data_dir: PathBuf,
    pub default_model: String,
}
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<crate::state::Store>,
    pub sessions: SessionManager,
    pub llm: SharedLlmProvider,
    pub tools: SharedToolProvider,
    pub config: Arc<RuntimeConfig>,
    pub events: EventBus,
}
pub struct AgentRuntime {
    state: AppState,
}
impl AgentRuntime {
    pub async fn new(
        config: RuntimeConfig,
        llm: SharedLlmProvider,
        tools: SharedToolProvider,
    ) -> Result<Self> {
        tokio::fs::create_dir_all(&config.data_dir)
            .await
            .with_context(|| format!("création {}", config.data_dir.display()))?;
        let store = Arc::new(
            crate::state::Store::open(&config.data_dir)
                .await
                .with_context(|| format!("ouverture du store {}", config.data_dir.display()))?,
        );
        let sessions = SessionManager::with_tool_provider(Arc::clone(&store), Arc::clone(&tools));
        Ok(Self {
            state: AppState {
                store,
                sessions,
                llm,
                tools,
                config: Arc::new(config),
                events: EventBus::new(),
            },
        })
    }
    pub fn state(&self) -> &AppState {
        &self.state
    }
    pub async fn shutdown(&self) {
        let store = Arc::clone(&self.state.store);
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, store.cancel_all()).await {
            Ok(_) => tracing::info!("tours actifs annulés"),
            Err(_) => tracing::warn!(
                timeout_secs = SHUTDOWN_TIMEOUT.as_secs(),
                "timeout pendant l'arrêt gracieux"
            ),
        }
    }
}
#[cfg(test)]
#[path = "test/runtime.rs"]
mod tests;
