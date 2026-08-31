//! Provider-neutral runtime composition root.
use crate::events::EventBus;
use crate::execution::{AgentLoopConfig, TurnManager, TurnService};
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
    pub turns: TurnManager,
    pub turn_service: TurnService,
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
            .with_context(|| format!("failed to create {}", config.data_dir.display()))?;
        let store = Arc::new(
            crate::state::Store::open(&config.data_dir)
                .await
                .with_context(|| format!("failed to open store {}", config.data_dir.display()))?,
        );
        let sessions = SessionManager::with_tool_provider(Arc::clone(&store), Arc::clone(&tools));
        let turn_service = TurnService::new(
            Arc::clone(&store),
            Arc::clone(&llm),
            Arc::clone(&tools),
            AgentLoopConfig::default(),
        );
        Ok(Self {
            state: AppState {
                store,
                sessions,
                turns: TurnManager::new(),
                turn_service,
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
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, self.state.turns.cancel_all_and_wait()).await {
            Ok(Ok(count)) => {
                tracing::info!(cancelled_turns = count, "active turns shutdown completed")
            }
            Ok(Err(error)) => tracing::warn!(%error, "failed to shut down active turns"),
            Err(_) => tracing::warn!(
                timeout_secs = SHUTDOWN_TIMEOUT.as_secs(),
                "timeout during graceful shutdown"
            ),
        }
    }
}
#[cfg(test)]
#[path = "test/runtime.rs"]
mod tests;
