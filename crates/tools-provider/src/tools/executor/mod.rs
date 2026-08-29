//! Deterministic tool executor with provider-local execution and semantic UI events.
mod mapping;
mod notifications;
mod permission;
mod terminal;

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{SessionId, ToolCallId, ToolCallStatus};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::{ToolCallRequest, ToolProvider, ToolUiKind, ToolUiModel, TurnEventSink};
use serde_json::{Map, Value};
use tokio::sync::watch;

use super::contracts::{ToolCancellation, ToolPermissionMode};
use super::lifecycle::{
    bind_session_cancellation, session_cancelled, unbind_session_cancellation,
    wait_for_session_cancel, ToolLifecycle, ToolLifecycleState,
};
use super::sandbox::RiskLevel;
use super::tool_ux::{classify_risk, result_update, ToolInfo};

pub use mapping::map_stop_reason;
pub use notifications::{emit_error_chunk, safe_session_update};
pub use permission::{PermissionKind, PermissionRequest, PermissionResult};

fn tool_ui_kind(name: &str) -> ToolUiKind {
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

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_ok: bool,
    pub executed: bool,
}
impl ToolResult {
    pub fn err(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_ok: false, executed: false }
    }
}

#[derive(Debug)]
pub struct ExecutionOutcome {
    pub result: ToolResult,
    pub terminal_id: Option<String>,
    pub terminal_meta: Option<Map<String, Value>>,
    pub cancelled: bool,
}

struct TerminalFinish<'a> {
    call_id: &'a ToolCallId,
    lifecycle: &'a mut ToolLifecycle,
    tool_name: &'a str,
    arguments: &'a Value,
    content: String,
    is_ok: bool,
    cancelled: bool,
    reason: Option<&'a str>,
    terminal_id: Option<&'a str>,
    terminal_meta: Option<Map<String, Value>>,
}

pub struct ToolExecutor<'a> {
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) registry: &'a dyn ToolProvider,
    pub(crate) cwd: &'a Path,
    pub(crate) additional_dirs: &'a [PathBuf],
    pub(crate) get_mode: &'a (dyn Fn() -> ToolPermissionMode + Send + Sync),
    pub(crate) cancellation: watch::Receiver<bool>,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(
        cx: &'a ConnectionTo<Client>,
        session_id: &'a SessionId,
        registry: &'a dyn ToolProvider,
        cwd: &'a Path,
        additional_dirs: &'a [PathBuf],
        get_mode: &'a (dyn Fn() -> ToolPermissionMode + Send + Sync),
        cancellation: watch::Receiver<bool>,
    ) -> Self {
        bind_session_cancellation(session_id.0.as_ref(), ToolCancellation::from_receiver(cancellation.clone()));
        Self { cx, session_id, registry, cwd, additional_dirs, get_mode, cancellation }
    }

    pub async fn execute_with_call_id_and_events(
        &self,
        call_id: ToolCallId,
        tool_name: &str,
        arguments: &Value,
        semantic: &mut dyn TurnEventSink,
    ) -> ToolResult {
        self.execute_inner(call_id, tool_name, arguments, Some(semantic)).await
    }

    pub async fn execute_with_call_id(
        &self,
        call_id: ToolCallId,
        tool_name: &str,
        arguments: &Value,
    ) -> ToolResult {
        self.execute_inner(call_id, tool_name, arguments, None).await
    }

    pub async fn execute(&self, tool_name: &str, arguments: &Value) -> ToolResult {
        self.execute_with_call_id(ToolCallId::from(format!("call_{}", uuid::Uuid::new_v4().simple())), tool_name, arguments).await
    }

    fn semantic_ui(&self, tool_name: &str, arguments: &Value, info: &ToolInfo) -> ToolUiModel {
        ToolUiModel::pending(
            tool_ui_kind(tool_name),
            info.title.clone(),
            info.title.clone(),
            super::tool_ux::bounded_raw_input(arguments),
        )
        .with_content(info.content.clone())
        .with_locations(info.locations.clone())
    }

    fn finish_terminal(&self, r: TerminalFinish<'_>, mut semantic: Option<&mut dyn TurnEventSink>) -> ToolResult {
        let TerminalFinish { call_id, lifecycle, tool_name, arguments, content, is_ok, cancelled, reason, terminal_id, terminal_meta } = r;
        let executed = matches!(lifecycle.state(), ToolLifecycleState::Executing);
        let envelope = lifecycle.finish_with_result(tool_name, content.clone(), is_ok, cancelled).expect("terminal tool path must finalize exactly once");
        let rendered = result_update(tool_name, arguments, &envelope.content, envelope.status == ToolCallStatus::Completed, self.cwd, terminal_id);
        if let Some(e) = semantic.as_mut() {
            let info = ToolInfo::build(tool_name, arguments, self.cwd, terminal_id);
            let mut ui = ToolUiModel::pending(
                tool_ui_kind(tool_name),
                info.title.clone(),
                info.title,
                super::tool_ux::bounded_raw_input(arguments),
            );
            ui = if cancelled { ui.cancelled(Some(serde_json::json!({ "text": content }))) } else { ui.completed(envelope.status == ToolCallStatus::Completed, Some(serde_json::json!({ "text": content }))) };
            ui = ui.with_content(info.content.into_iter().chain(rendered.content).collect()).with_locations(rendered.locations);
            e.tool_result_received(call_id.to_string(), content.clone(), Some(ui));
        }
        let _ = reason;
        let _ = terminal_meta;
        ToolResult { content, is_ok: envelope.status == ToolCallStatus::Completed, executed }
    }

    async fn execute_inner(&self, call_id: ToolCallId, tool_name: &str, arguments: &Value, mut semantic: Option<&mut dyn TurnEventSink>) -> ToolResult {
        let info = ToolInfo::build(tool_name, arguments, self.cwd, None);
        let mut lifecycle = ToolLifecycle::new();
        let ui = self.semantic_ui(tool_name, arguments, &info);
        if let Some(e) = semantic.as_mut() {
            e.tool_call_requested(call_id.to_string(), tool_name.to_owned(), Some(ui.clone()));
        }
        if *self.cancellation.borrow() {
            return self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: "outil annulé avant son démarrage".into(), is_ok: false, cancelled: true, reason: Some("cancelled"), terminal_id: None, terminal_meta: None }, semantic);
        }
        let mode = (self.get_mode)();
        let needs_permission = matches!(info.kind, ToolUiKind::FileWrite | ToolUiKind::FileEdit | ToolUiKind::ReplaceInFile | ToolUiKind::Shell)
            && match mode {
                ToolPermissionMode::BypassPermissions => false,
                ToolPermissionMode::AcceptEdits => matches!(info.kind, ToolUiKind::Shell) && classify_risk(tool_name, arguments) >= RiskLevel::High,
                ToolPermissionMode::Default => true,
            };
        if needs_permission {
            lifecycle.transition(ToolLifecycleState::Permission).expect("pending -> permission must be legal");
            if let Some(e) = semantic.as_mut() { e.permission_requested(call_id.to_string()); }
            let request = PermissionRequest::from_tool_call(tool_name, arguments, self.cwd);
            match self.request_permission(&request, &call_id).await {
                PermissionResult::Allow => {
                    if *self.cancellation.borrow() {
                        return self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: format!("{} ({}) annulé avant le démarrage de l'exécution.", request.kind.label(), request.summary), is_ok: false, cancelled: true, reason: Some("cancelled"), terminal_id: None, terminal_meta: None }, semantic);
                    }
                    lifecycle.transition(ToolLifecycleState::Executing).expect("permission -> executing must be legal");
                    if let Some(e) = semantic.as_mut() { e.tool_execution_started(call_id.to_string(), Some(ui.clone())); }
                }
                PermissionResult::Reject => return self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: format!("{} ({}) refusé par l'utilisateur.", request.kind.label(), request.summary), is_ok: false, cancelled: false, reason: Some("user-rejected"), terminal_id: None, terminal_meta: None }, semantic),
                PermissionResult::Cancelled => return self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: format!("{} ({}) annulé pendant la demande d'autorisation.", request.kind.label(), request.summary), is_ok: false, cancelled: true, reason: Some("cancelled"), terminal_id: None, terminal_meta: None }, semantic),
                PermissionResult::TransportError(error) => return self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: format!("Échec de la demande de permission ACP : {error}"), is_ok: false, cancelled: false, reason: Some("permission-error"), terminal_id: None, terminal_meta: None }, semantic),
            }
        } else {
            lifecycle.transition(ToolLifecycleState::Executing).expect("pending -> executing must be legal");
            if let Some(e) = semantic.as_mut() { e.tool_execution_started(call_id.to_string(), Some(ui)); }
        }

        let outcome = if tool_name == "shell_exec" {
            match self.execute_shell_via_acp_terminal(arguments, &call_id, &lifecycle).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    tracing::debug!(session = %self.session_id, error = %error, "terminal ACP indisponible avant exécution, fallback provider");
                    self.execute_registry(&call_id, tool_name, arguments).await
                }
            }
        } else {
            self.execute_registry(&call_id, tool_name, arguments).await
        };

        self.finish_terminal(TerminalFinish { call_id: &call_id, lifecycle: &mut lifecycle, tool_name, arguments, content: outcome.result.content, is_ok: outcome.result.is_ok, cancelled: outcome.cancelled, reason: if outcome.cancelled { Some("cancelled") } else { None }, terminal_id: outcome.terminal_id.as_deref(), terminal_meta: outcome.terminal_meta }, semantic)
    }

    async fn execute_registry(&self, call_id: &ToolCallId, tool_name: &str, arguments: &Value) -> ExecutionOutcome {
        let request = ToolCallRequest { call_id: call_id.to_string(), session_id: self.session_id.0.to_string(), name: tool_name.to_owned(), arguments: arguments.clone(), cwd: self.cwd.to_path_buf(), additional_dirs: self.additional_dirs.to_vec(), cancellation: self.cancellation.clone() };
        let result = tokio::select! {
            value = self.registry.call(request) => value,
            _ = wait_for_session_cancel(self.session_id.0.as_ref()) => return ExecutionOutcome { result: ToolResult::err("outil annulé pendant son exécution"), terminal_id: None, terminal_meta: None, cancelled: true },
        };
        let cancelled = session_cancelled(self.session_id.0.as_ref()) || *self.cancellation.borrow();
        ExecutionOutcome { result: ToolResult { content: result.content, is_ok: result.is_ok, executed: result.executed }, terminal_id: None, terminal_meta: None, cancelled }
    }
}

impl Drop for ToolExecutor<'_> {
    fn drop(&mut self) { unbind_session_cancellation(self.session_id.0.as_ref()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_kind_mapping() {
        assert_eq!(PermissionKind::Write.label(), "write");
        assert_eq!(PermissionKind::Execute.label(), "execute");
    }
    #[test]
    fn stop_reason_mapping() {
        use agent_client_protocol::schema::v1::StopReason;
        assert_eq!(map_stop_reason(Some("length")), StopReason::MaxTokens);
        assert_eq!(map_stop_reason(Some("content_filter")), StopReason::Refusal);
        assert_eq!(map_stop_reason(None), StopReason::EndTurn);
    }
    #[test]
    fn cancelled_terminal_preserves_partial_output() {
        assert_eq!(terminal::terminal_output_text(("partial output".into(), false)), "partial output");
        assert_eq!(terminal::terminal_output_text(("partial output".into(), true)), "partial output\n… (sortie tronquée par le client ACP)");
    }
    #[test]
    fn empty_cancelled_terminal_output_stays_empty() {
        assert!(terminal::terminal_output_text(("   ".into(), false)).is_empty());
    }
    #[test]
    fn terminal_metadata_shape() {
        let meta = terminal::terminal_lifecycle_meta("term-1", Some("hello"), Some((Some(0), None)));
        assert_eq!(meta["terminal_info"]["terminal_id"], "term-1");
        assert_eq!(meta["terminal_output"]["data"], "hello");
        assert_eq!(meta["terminal_exit"]["exit_code"], 0);
        assert!(meta["terminal_exit"]["signal"].is_null());
    }
}
