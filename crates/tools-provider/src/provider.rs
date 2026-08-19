//! Session-aware builtin/MCP implementation of the runtime `ToolProvider` contract.
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use agent_runtime::{ToolCallRequest, ToolCallResult, ToolProvider, ToolServerConfig, ToolUiModel};

use crate::tools::contracts::ToolCancellation;
use crate::tools::lifecycle::{bind_session_cancellation, unbind_session_cancellation};
use crate::tools::mcp::{McpCatalog, McpServerConfig as ProviderMcpServerConfig};
use crate::tools::registry::ToolRegistry;
use crate::tools::ui;

struct ProviderState {
    fallback: Arc<ToolRegistry>,
    sessions: RwLock<std::collections::HashMap<String, Arc<ToolRegistry>>>,
}

#[derive(Clone)]
pub struct DefaultToolProvider {
    state: Arc<ProviderState>,
    registry: Arc<ToolRegistry>,
}

impl DefaultToolProvider {
    pub fn new(fallback: ToolRegistry) -> Self {
        let fallback = Arc::new(fallback);
        Self {
            state: Arc::new(ProviderState {
                fallback: Arc::clone(&fallback),
                sessions: RwLock::new(std::collections::HashMap::new()),
            }),
            registry: fallback,
        }
    }

    pub async fn from_env() -> anyhow::Result<Self> {
        let mut registry = ToolRegistry::builtin();
        let catalog = McpCatalog::from_env().await?;
        if catalog.has_tools() {
            registry.register_mcp(Arc::new(catalog));
        }
        Ok(Self::new(registry))
    }
}

#[async_trait::async_trait]
impl ToolProvider for DefaultToolProvider {
    async fn for_session(&self, session_id: &str) -> Arc<dyn ToolProvider> {
        let registry = self
            .state
            .sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.state.fallback));
        Arc::new(Self {
            state: Arc::clone(&self.state),
            registry,
        })
    }

    async fn configure_session(
        &self,
        session_id: &str,
        _cwd: PathBuf,
        servers: Vec<ToolServerConfig>,
    ) -> Result<(), String> {
        if servers.is_empty() {
            self.clear_session(session_id).await;
            return Ok(());
        }

        let configs = servers
            .into_iter()
            .map(ProviderMcpServerConfig::from)
            .collect::<Vec<_>>();
        let catalog = McpCatalog::from_configs(configs)
            .await
            .map_err(|error| error.to_string())?;
        let mut registry = ToolRegistry::builtin();
        registry.register_mcp(Arc::new(catalog));
        self.state
            .sessions
            .write()
            .await
            .insert(session_id.to_owned(), Arc::new(registry));
        Ok(())
    }

    async fn clear_session(&self, session_id: &str) {
        self.state.sessions.write().await.remove(session_id);
    }

    fn definitions(&self) -> Vec<Value> {
        self.registry.definitions()
    }

    fn prompt_fragment(&self) -> Option<String> {
        crate::tools::prompt::tools_section(&self.registry)
    }

    fn has_tools(&self) -> bool {
        self.registry.has_tools()
    }

    fn ui_model(&self, name: &str, arguments: &Value) -> Option<ToolUiModel> {
        Some(ui::pending(name, arguments))
    }

    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        let cancellation = ToolCancellation::from_receiver(request.cancellation.clone());
        bind_session_cancellation(&request.session_id, cancellation);

        let result = match self
            .registry
            .call_async(
                &request.name,
                &request.arguments,
                &request.cwd,
                &request.additional_dirs,
            )
            .await
        {
            Some(crate::tools::registry::ToolResult::Ok(content)) => ToolCallResult {
                ui: Some(ui::completed(&request.name, &request.arguments, &content, true)),
                content,
                is_ok: true,
                executed: true,
            },
            Some(crate::tools::registry::ToolResult::Err(content)) => ToolCallResult {
                ui: Some(ui::completed(&request.name, &request.arguments, &content, false)),
                content,
                is_ok: false,
                executed: true,
            },
            None => ToolCallResult {
                ui: Some(ui::completed(
                    &request.name,
                    &request.arguments,
                    &format!("Outil inconnu : {}", request.name),
                    false,
                )),
                content: format!("Outil inconnu : {}", request.name),
                is_ok: false,
                executed: false,
            },
        };

        unbind_session_cancellation(&request.session_id);
        result
    }
}
