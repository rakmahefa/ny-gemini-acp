//! Deterministic tool executor with Claude-style ACP UX and real ACP permissions.
//!
//! Internal lifecycle: `pending -> permission -> executing -> completed|failed|cancelled`.
//! ACP v1 has no cancelled tool status, so Cancelled is projected to Failed on
//! the wire and explained in `_meta`.

mod mapping;
mod notifications;
mod permission;
mod terminal;

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{SessionId, ToolCallId, ToolCallStatus};
use agent_client_protocol::{Client, ConnectionTo};
use serde_json::{Map, Value};

use crate::state::SessionMode;

use super::lifecycle::{
    session_cancelled, wait_for_session_cancel, ToolLifecycle, ToolLifecycleState,
};
use super::registry::ToolRegistry;
use super::tool_ux::{classify_risk, result_update, ToolInfo};

pub use mapping::map_stop_reason;
pub use notifications::{emit_error_chunk, safe_session_update};
pub use permission::{PermissionKind, PermissionRequest, PermissionResult};

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_ok: bool,
}

impl ToolResult {
    pub fn err(content: impl Into<String>) -> Self {
        Self { content: content.into(), is_ok: false }
    }
}

#[derive(Debug)]
struct ExecutionOutcome {
    result: ToolResult,
    terminal_id: Option<String>,
    terminal_meta: Option<Map<String, Value>>,
    cancelled: bool,
}

pub struct ToolExecutor<'a> {
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) registry: &'a ToolRegistry,
    pub(crate) cwd: &'a Path,
    pub(crate) additional_dirs: &'a [PathBuf],
    pub(crate) get_mode: &'a (dyn Fn() -> SessionMode + Send + Sync),
}

impl<'a> ToolExecutor<'a> {
    pub fn new(
        cx: &'a ConnectionTo<Client>,
        session_id: &'a SessionId,
        registry: &'a ToolRegistry,
        cwd: &'a Path,
        additional_dirs: &'a [PathBuf],
        get_mode: &'a (dyn Fn() -> SessionMode + Send + Sync),
    ) -> Self {
        Self { cx, session_id, registry, cwd, additional_dirs, get_mode }
    }

    pub async fn execute(&self, tool_name: &str, arguments: &Value) -> ToolResult {
        let call_id = ToolCallId::from(format!("call_{}", uuid::Uuid::new_v4().simple()));
        let info = ToolInfo::build(tool_name, arguments, self.cwd, None);
        let mut lifecycle = ToolLifecycle::new();
        self.emit_tool_call(&call_id, &info, &lifecycle, arguments);

        if session_cancelled(self.session_id.0.as_ref()) {
            lifecycle.cancel().expect("pending -> cancelled must be legal");
            let message = "outil annulé avant son démarrage";
            let meta = mapping::lifecycle_meta(tool_name, &lifecycle, Some("cancelled"), None);
            self.emit_failed(&call_id, message, arguments, tool_name, Some(meta));
            return ToolResult::err(message);
        }

        let mode = (self.get_mode)();
        let needs_permission = match info.kind {
            agent_client_protocol::schema::v1::ToolKind::Edit
            | agent_client_protocol::schema::v1::ToolKind::Execute => match mode {
                SessionMode::BypassPermissions => false,
                SessionMode::AcceptEdits => {
                    info.kind == agent_client_protocol::schema::v1::ToolKind::Execute
                        && classify_risk(tool_name, arguments)
                            >= super::sandbox::RiskLevel::High
                }
                SessionMode::Default => true,
            },
            _ => false,
        };

        if needs_permission {
            lifecycle
                .transition(ToolLifecycleState::Permission)
                .expect("pending -> permission must be legal");
            self.emit_lifecycle(&call_id, &lifecycle, tool_name);
            let request = PermissionRequest::from_tool_call(tool_name, arguments, self.cwd);
            match self.request_permission(&request, &call_id).await {
                PermissionResult::Allow => {
                    if session_cancelled(self.session_id.0.as_ref()) {
                        lifecycle
                            .transition(ToolLifecycleState::Cancelled)
                            .expect("permission -> cancelled must be legal");
                        let message = format!(
                            "{} ({}) annulé avant le démarrage de l'exécution.",
                            request.kind.label(), request.summary
                        );
                        let meta = mapping::lifecycle_meta(
                            tool_name,
                            &lifecycle,
                            Some("cancelled"),
                            None,
                        );
                        self.emit_failed(&call_id, &message, arguments, tool_name, Some(meta));
                        return ToolResult::err(message);
                    }
                    lifecycle
                        .transition(ToolLifecycleState::Executing)
                        .expect("permission -> executing must be legal");
                    self.emit_lifecycle(&call_id, &lifecycle, tool_name);
                }
                PermissionResult::Reject => {
                    lifecycle
                        .transition(ToolLifecycleState::Failed)
                        .expect("permission -> failed must be legal");
                    let message = format!(
                        "{} ({}) refusé par l'utilisateur.",
                        request.kind.label(), request.summary
                    );
                    let meta = mapping::lifecycle_meta(
                        tool_name,
                        &lifecycle,
                        Some("user-rejected"),
                        None,
                    );
                    self.emit_failed(&call_id, &message, arguments, tool_name, Some(meta));
                    return ToolResult::err(message);
                }
                PermissionResult::Cancelled => {
                    lifecycle
                        .transition(ToolLifecycleState::Cancelled)
                        .expect("permission -> cancelled must be legal");
                    let message = format!(
                        "{} ({}) annulé pendant la demande d'autorisation.",
                        request.kind.label(), request.summary
                    );
                    let meta = mapping::lifecycle_meta(
                        tool_name,
                        &lifecycle,
                        Some("cancelled"),
                        None,
                    );
                    self.emit_failed(&call_id, &message, arguments, tool_name, Some(meta));
                    return ToolResult::err(message);
                }
                PermissionResult::TransportError(error) => {
                    lifecycle
                        .transition(ToolLifecycleState::Failed)
                        .expect("permission -> failed must be legal");
                    let message = format!("Échec de la demande de permission ACP : {error}");
                    let meta = mapping::lifecycle_meta(
                        tool_name,
                        &lifecycle,
                        Some("permission-error"),
                        None,
                    );
                    self.emit_failed(&call_id, &message, arguments, tool_name, Some(meta));
                    return ToolResult::err(message);
                }
            }
        } else {
            if session_cancelled(self.session_id.0.as_ref()) {
                lifecycle.cancel().expect("pending -> cancelled must be legal");
                let message = "outil annulé avant son exécution";
                let meta = mapping::lifecycle_meta(tool_name, &lifecycle, Some("cancelled"), None);
                self.emit_failed(&call_id, message, arguments, tool_name, Some(meta));
                return ToolResult::err(message);
            }
            lifecycle
                .transition(ToolLifecycleState::Executing)
                .expect("pending -> executing must be legal");
            self.emit_lifecycle(&call_id, &lifecycle, tool_name);
        }

        let outcome = if tool_name == "shell_exec" {
            match self.execute_shell_via_acp_terminal(arguments, &call_id).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    tracing::debug!(
                        session=%self.session_id,
                        error=%error,
                        "terminal ACP indisponible avant exécution, fallback shell local"
                    );
                    self.execute_registry(tool_name, arguments).await
                }
            }
        } else {
            self.execute_registry(tool_name, arguments).await
        };

        // The lifecycle is now the single source of truth for the terminal
        // state and the canonical result payload. The executor must not decide
        // Completed/Failed/Cancelled independently anymore.
        let envelope = lifecycle
            .finish_with_result(
                tool_name,
                outcome.result.content.clone(),
                outcome.result.is_ok,
                outcome.cancelled,
            )
            .expect("executing tool must finish exactly once in a terminal state");

        debug_assert_eq!(envelope.status, lifecycle.status());

        let rendered = result_update(
            tool_name,
            arguments,
            &envelope.content,
            envelope.status == ToolCallStatus::Completed,
            self.cwd,
            outcome.terminal_id.as_deref(),
        );
        let meta = mapping::lifecycle_meta(
            tool_name,
            &lifecycle,
            if outcome.cancelled { Some("cancelled") } else { None },
            outcome.terminal_meta,
        );
        self.emit_update(
            &call_id,
            envelope.status,
            rendered.content,
            rendered.locations,
            Some(meta),
        );
        outcome.result
    }

    async fn execute_registry(&self, tool_name: &str, arguments: &Value) -> ExecutionOutcome {
        let result = tokio::select! {
            value = self.registry.call_async(tool_name, arguments, self.cwd, self.additional_dirs) => value,
            _ = wait_for_session_cancel(self.session_id.0.as_ref()) => return ExecutionOutcome { result: ToolResult::err("outil annulé pendant son exécution"), terminal_id: None, terminal_meta: None, cancelled: true },
        };
        let cancelled = session_cancelled(self.session_id.0.as_ref());
        match result {
            Some(result) => ExecutionOutcome { result: mapping::registry_result(result), terminal_id: None, terminal_meta: None, cancelled },
            None => ExecutionOutcome { result: ToolResult::err(format!("Outil inconnu : {tool_name}")), terminal_id: None, terminal_meta: None, cancelled },
        }
    }
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
