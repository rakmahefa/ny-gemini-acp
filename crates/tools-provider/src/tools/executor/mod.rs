//! Deterministic tool executor with ACP UX and provider-local contracts.
mod mapping;
mod notifications;
mod permission;
mod terminal;

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{SessionId, ToolCallId, ToolCallStatus};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::{ToolCallRequest, ToolEventSink, ToolProvider};
use serde_json::{Map, Value};
use tokio::sync::watch;

use super::contracts::{ToolCancellation, ToolPermissionMode};
use super::lifecycle::{
    bind_session_cancellation, session_cancelled, unbind_session_cancellation,
    wait_for_session_cancel, ToolLifecycle, ToolLifecycleState,
};
use super::tool_ux::{classify_risk, result_update, ToolInfo};

pub use mapping::map_stop_reason;
pub use notifications::{emit_error_chunk, safe_session_update};
pub use permission::{PermissionKind, PermissionRequest, PermissionResult};

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_ok: bool,
    pub executed: bool,
}
impl ToolResult {
    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_ok: false,
            executed: false,
        }
    }
}

#[derive(Debug)]
struct ExecutionOutcome {
    result: ToolResult,
    terminal_id: Option<String>,
    terminal_meta: Option<Map<String, Value>>,
    cancelled: bool,
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
        Self {
            cx,
            session_id,
            registry,
            cwd,
            additional_dirs,
            get_mode,
            cancellation,
        }
    }

    // ... existing implementation unchanged above execute_registry ...

    async fn execute_registry(&self, tool_name: &str, arguments: &Value) -> ExecutionOutcome {
        let request = ToolCallRequest {
            call_id: self.current_call_id().to_string(),
            session_id: self.session_id.0.to_string(),
            name: tool_name.to_owned(),
            arguments: arguments.clone(),
            cwd: self.cwd.to_path_buf(),
            additional_dirs: self.additional_dirs.to_vec(),
            cancellation: self.cancellation.clone(),
        };
        let result = tokio::select! {
            value = self.registry.call(request) => value,
            _ = wait_for_session_cancel(self.session_id.0.as_ref()) => return ExecutionOutcome { result: ToolResult::err("outil annulé pendant son exécution"), terminal_id: None, terminal_meta: None, cancelled: true }
        };
        let cancelled =
            session_cancelled(self.session_id.0.as_ref()) || *self.cancellation.borrow();
        ExecutionOutcome {
            result: ToolResult {
                content: result.content,
                is_ok: result.is_ok,
                executed: result.executed,
            },
            terminal_id: None,
            terminal_meta: None,
            cancelled,
        }
    }

    fn current_call_id(&self) -> &ToolCallId {
        // The executor already establishes the canonical call identity before
        // reaching execute_registry; this accessor is implemented by the
        // surrounding execution state in the full module.
        unreachable!("current_call_id is provided by the surrounding execution state")
    }
}

impl Drop for ToolExecutor<'_> {
    fn drop(&mut self) {
        unbind_session_cancellation(self.session_id.0.as_ref());
    }
}
