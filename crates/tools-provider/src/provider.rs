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
        "FollowUp" => ToolUiKind::Generic,
        _ => ToolUiKind::Generic,
    }
}

fn terminal_id(name: &str, call_id: &str) -> Option<String> {
    (name == "shell_exec" && !call_id.trim().is_empty())
        .then(|| format!("terminal-{call_id}"))
}

fn rich_values<T: serde::Serialize>(values: &[T]) -> Vec<Value> {
    values.iter().filter_map(|value| serde_json::to_value(value).ok()).collect()
}

fn presentation_info(call_id: &str, name: &str, arguments: &Value, cwd: &Path) -> ToolInfo {
    let terminal = terminal_id(name, call_id);
    ToolInfo::build(name, arguments, cwd, terminal.as_deref())
}

fn pending_ui(call_id: &str, name: &str, arguments: &Value, cwd: &Path) -> ToolUiModel {
    let info = presentation_info(call_id, name, arguments, cwd);
    ToolUiModel::pending(ui_kind(name), info.title.clone(), info.title, bounded_raw_input(arguments))
        .with_content(rich_values(&info.content))
        .with_locations(rich_values(&info.locations))
}

fn completed_ui_from_info(
    call_id: &str,
    name: &str,
    arguments: &Value,
    content: &str,
    is_ok: bool,
    cwd: &Path,
    info: &ToolInfo,
) -> ToolUiModel {
    let terminal = terminal_id(name, call_id);
    let rendered = result_update(name, arguments, content, is_ok, cwd, terminal.as_deref());

    // Contract visuel: l'Input appartient uniquement au ToolCall initial.
    // Le Diff reste persistant au résultat; Terminal est réémis uniquement par ResultUpdate.
    let mut rich_content = info
        .content
        .iter()
        .filter_map(|item| {
            let value = serde_json::to_value(item).ok()?;
            let kind = value.get("type").and_then(Value::as_str)?;
            (kind != "content" && kind != "terminal").then_some(value)
        })
        .collect::<Vec<_>>();
    rich_content.extend(rich_values(&rendered.content));

    let locations = rendered
        .locations
        .iter()
        .filter(|location| location.path.exists())
        .filter_map(|location| serde_json::to_value(location).ok())
        .collect::<Vec<_>>();

    ToolUiModel::pending(
        ui_kind(name),
        info.title.clone(),
        info.title.clone(),
        bounded_raw_input(arguments),
    )
    .completed(is_ok, Some(json!({ "text": content })))
    .with_content(rich_content)
    .with_locations(locations)
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
    ) -> Result<(), String> {
        if servers.is_empty() {
            self.state.sessions.write().await.insert(
                session_id.to_owned(),
                SessionToolBinding { registry: Arc::clone(&self.state.fallback), cwd },
            );
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
        self.state.sessions.write().await.insert(
            session_id.to_owned(),
            SessionToolBinding { registry: Arc::new(registry), cwd },
        );
        Ok(())
    }

    async fn clear_session(&self, session_id: &str) { self.state.sessions.write().await.remove(session_id); }
    fn definitions(&self) -> Vec<Value> { self.registry.definitions() }
    fn prompt_fragment(&self) -> Option<String> { crate::tools::prompt::tools_section(&self.registry) }
    fn has_tools(&self) -> bool { self.registry.has_tools() }

    fn ui_model(&self, call_id: &str, name: &str, arguments: &Value) -> Option<ToolUiModel> {
        let cwd = self.cwd.as_deref().unwrap_or_else(|| Path::new("."));
        Some(pending_ui(call_id, name, arguments, cwd))
    }

    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        let cancellation = ToolCancellation::from_receiver(request.cancellation.clone());
        bind_session_cancellation(&request.session_id, cancellation);
        let info = presentation_info(&request.call_id, &request.name, &request.arguments, &request.cwd);

        let result = match self
            .registry
            .call_async(&request.name, &request.arguments, &request.cwd, &request.additional_dirs)
            .await
        {
            Some(crate::tools::registry::ToolResult::Ok(content)) => ToolCallResult {
                ui: Some(completed_ui_from_info(&request.call_id, &request.name, &request.arguments, &content, true, &request.cwd, &info)),
                content,
                is_ok: true,
                executed: true,
            },
            Some(crate::tools::registry::ToolResult::Err(content)) => ToolCallResult {
                ui: Some(completed_ui_from_info(&request.call_id, &request.name, &request.arguments, &content, false, &request.cwd, &info)),
                content,
                is_ok: false,
                executed: true,
            },
            None => {
                let content = format!("Outil inconnu : {}", request.name);
                ToolCallResult {
                    ui: Some(completed_ui_from_info(&request.call_id, &request.name, &request.arguments, &content, false, &request.cwd, &info)),
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
