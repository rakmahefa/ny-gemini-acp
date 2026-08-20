//! Session-aware builtin/MCP implementation of the runtime `ToolProvider` contract.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::RwLock;

use agent_runtime::{ToolCallRequest, ToolCallResult, ToolProvider, ToolServerConfig, ToolUiKind, ToolUiModel};

use crate::tools::contracts::ToolCancellation;
use crate::tools::lifecycle::{bind_session_cancellation, unbind_session_cancellation};
use crate::tools::mcp::{McpCatalog, McpServerConfig as ProviderMcpServerConfig};
use crate::tools::registry::ToolRegistry;
use crate::tools::tool_ux::{bounded_raw_input, result_update, ToolInfo};

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

fn ui_kind(name: &str) -> ToolUiKind {
    match name {
        "file_read" => ToolUiKind::FileRead,
        "file_write" => ToolUiKind::FileWrite,
        "file_edit" => ToolUiKind::FileEdit,
        "glob" => ToolUiKind::Glob,
        "list_directory" => ToolUiKind::DirectoryList,
        "search" => ToolUiKind::Search,
        "search_and_read" => ToolUiKind::SearchAndRead,
        "shell_exec" => ToolUiKind::Shell,
        "replace_in_file" => ToolUiKind::ReplaceInFile,
        "AskUserQuestion" => ToolUiKind::AskUserQuestion,
        _ => ToolUiKind::Generic,
    }
}

fn completed_ui(
    name: &str,
    arguments: &Value,
    content: &str,
    is_ok: bool,
    cwd: &Path,
) -> ToolUiModel {
    let rendered = result_update(name, arguments, content, is_ok, cwd, None);
    let output = json!({
        "text": content,
        "content": rendered.content,
        "locations": rendered.locations,
    });

    let info = ToolInfo::build(name, arguments, cwd, None);
    ToolUiModel::pending(
        ui_kind(name),
        info.title.clone(),
        info.title,
        bounded_raw_input(arguments),
    )
    .completed(is_ok, Some(output))
}

fn pending_ui_with_rich_content(name: &str, arguments: &Value) -> ToolUiModel {
    let info = ToolInfo::build(name, arguments, Path::new("."), None);
    let output = json!({
        "content": info.content,
        "locations": info.locations,
    });

    let mut ui = ToolUiModel::pending(
        ui_kind(name),
        info.title.clone(),
        info.title,
        bounded_raw_input(arguments),
    );
    ui.output = Some(output);
    ui
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
        Some(pending_ui_with_rich_content(name, arguments))
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
                ui: Some(completed_ui(
                    &request.name,
                    &request.arguments,
                    &content,
                    true,
                    &request.cwd,
                )),
                content,
                is_ok: true,
                executed: true,
            },
            Some(crate::tools::registry::ToolResult::Err(content)) => ToolCallResult {
                ui: Some(completed_ui(
                    &request.name,
                    &request.arguments,
                    &content,
                    false,
                    &request.cwd,
                )),
                content,
                is_ok: false,
                executed: true,
            },
            None => {
                let content = format!("Outil inconnu : {}", request.name);
                ToolCallResult {
                    ui: Some(completed_ui(
                        &request.name,
                        &request.arguments,
                        &content,
                        false,
                        &request.cwd,
                    )),
                    content,
                    is_ok: false,
                    executed: false,
                }
            }
        };

        unbind_session_cancellation(&request.session_id);
        result
    }
}
