//! Session-aware builtin/MCP implementation of the runtime `ToolProvider` contract.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::RwLock;

use agent_runtime::{
    ToolCallRequest, ToolCallResult, ToolConfigurationError, ToolProvider, ToolServerConfig,
    ToolUiModel,
};

use crate::tools::mcp::{McpCatalog, McpError, McpServerConfig as ProviderMcpServerConfig};
use crate::tools::registry::ToolRegistry;
use crate::tools::tool_ux::{bounded_raw_input, result_update, ToolInfo};

struct SessionToolBinding {
    registry: Arc<ToolRegistry>,
    cwd: PathBuf,
}

struct ProviderState {
    fallback: Arc<ToolRegistry>,
    sessions: RwLock<std::collections::HashMap<String, SessionToolBinding>>,
}

#[derive(Clone)]
pub struct DefaultToolProvider {
    state: Arc<ProviderState>,
    registry: Arc<ToolRegistry>,
    cwd: Option<PathBuf>,
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
            cwd: None,
        }
    }

    pub async fn from_env() -> anyhow::Result<Self> {
        let mut registry = ToolRegistry::builtin();
        let catalog = McpCatalog::from_env().await?;
        if catalog.has_tools() {
            registry
                .register_mcp(Arc::new(catalog))
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        Ok(Self::new(registry))
    }
}

fn presentation_info(name: &str, arguments: &Value, cwd: &Path) -> ToolInfo {
    ToolInfo::build(name, arguments, cwd, None)
}

fn pending_ui(name: &str, arguments: &Value, cwd: &Path) -> ToolUiModel {
    let info = presentation_info(name, arguments, cwd);
    ToolUiModel::pending(
        crate::tools::tool_ux::tool_ui_kind(name),
        info.title.clone(),
        info.title,
        bounded_raw_input(arguments),
    )
    .with_content(info.content)
    .with_locations(info.locations)
}

fn completed_ui_from_info(
    name: &str,
    arguments: &Value,
    content: &str,
    is_ok: bool,
    cwd: &Path,
    info: &ToolInfo,
) -> ToolUiModel {
    let rendered = result_update(name, arguments, content, is_ok, cwd, None);
    let mut rich_content = info.content.clone();
    rich_content.extend(rendered.content);

    ToolUiModel::pending(
        crate::tools::tool_ux::tool_ui_kind(name),
        info.title.clone(),
        info.title.clone(),
        bounded_raw_input(arguments),
    )
    .completed(is_ok, Some(json!({ "text": content })))
    .with_content(rich_content)
    .with_locations(rendered.locations)
}

fn map_mcp_error(error: McpError) -> ToolConfigurationError {
    match error {
        McpError::Config(message) => ToolConfigurationError::InvalidConfiguration(message),
        McpError::Transport { transport, message } => {
            ToolConfigurationError::Transport { transport, message }
        }
        McpError::Protocol(message) => ToolConfigurationError::Protocol(message),
        McpError::Remote { code, message } => ToolConfigurationError::Remote { code, message },
        McpError::MessageTooLarge => ToolConfigurationError::MessageTooLarge,
        McpError::PaginationLimit => ToolConfigurationError::PaginationLimit,
    }
}

#[async_trait::async_trait]
impl ToolProvider for DefaultToolProvider {
    async fn for_session(&self, session_id: &str) -> Arc<dyn ToolProvider> {
        if let Some(binding) = self.state.sessions.read().await.get(session_id) {
            return Arc::new(Self {
                state: Arc::clone(&self.state),
                registry: Arc::clone(&binding.registry),
                cwd: Some(binding.cwd.clone()),
            });
        }
        Arc::new(Self {
            state: Arc::clone(&self.state),
            registry: Arc::clone(&self.state.fallback),
            cwd: None,
        })
    }

    async fn configure_session(
        &self,
        session_id: &str,
        cwd: PathBuf,
        servers: Vec<ToolServerConfig>,
    ) -> Result<(), ToolConfigurationError> {
        if servers.is_empty() {
            self.state.sessions.write().await.insert(
                session_id.to_owned(),
                SessionToolBinding {
                    registry: Arc::clone(&self.state.fallback),
                    cwd,
                },
            );
            return Ok(());
        }

        let configs = servers
            .into_iter()
            .map(ProviderMcpServerConfig::from)
            .collect::<Vec<_>>();
        let catalog = McpCatalog::from_configs(configs)
            .await
            .map_err(map_mcp_error)?;
        let mut registry = ToolRegistry::builtin();
        registry.register_mcp(Arc::new(catalog))?;
        self.state.sessions.write().await.insert(
            session_id.to_owned(),
            SessionToolBinding {
                registry: Arc::new(registry),
                cwd,
            },
        );
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

    fn ui_model(&self, _call_id: &str, name: &str, arguments: &Value) -> Option<ToolUiModel> {
        let cwd = self.cwd.as_deref().unwrap_or_else(|| Path::new("."));
        Some(pending_ui(name, arguments, cwd))
    }

    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        // D-17 : le propriétaire unique du bind/unbind de la clé de session
        // dans la carte de cancellation est `ToolExecutor` (bind dans `new`,
        // unbind dans `Drop`). Un re-bind ici volait puis détruisait ce
        // binding à la fin de l'appel, laissant la carte vide pendant que
        // l'executor avait encore besoin du canal d'annulation — et rien dans
        // `registry::call_async` ne lit cette carte. L'annulation du provider
        // passe par `request.cancellation` consommé par l'executor.
        let info = presentation_info(&request.name, &request.arguments, &request.cwd);

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
                ui: Some(completed_ui_from_info(
                    &request.name,
                    &request.arguments,
                    &content,
                    true,
                    &request.cwd,
                    &info,
                )),
                content,
                is_ok: true,
            },
            Some(crate::tools::registry::ToolResult::Err(content)) => ToolCallResult {
                ui: Some(completed_ui_from_info(
                    &request.name,
                    &request.arguments,
                    &content,
                    false,
                    &request.cwd,
                    &info,
                )),
                content,
                is_ok: false,
            },
            None => {
                let content = format!("Outil inconnu : {}", request.name);
                ToolCallResult {
                    ui: Some(completed_ui_from_info(
                        &request.name,
                        &request.arguments,
                        &content,
                        false,
                        &request.cwd,
                        &info,
                    )),
                    content,
                    is_ok: false,
                }
            }
        };

        result
    }
}
