//! Tool executor boundary: session-cancellation binding and ACP permission
//! requests.
//!
//! The former provider-local execution path (`execute*`, terminal ACP,
//! lifecycle envelopes) was dead code: production tool execution goes
//! `agent-runtime -> ToolProvider::call -> registry.call_async`. It was
//! removed (SPEC-P1-04/SPEC-P1-05); the ACP-terminal migration decision is
//! tracked in `docs/adr/0001-sandbox-execution.md`.
mod notifications;
mod permission;

use std::path::Path;

use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::{Client, ConnectionTo};
use tokio::sync::watch;

use super::contracts::ToolCancellation;
use super::lifecycle::{bind_session_cancellation, unbind_session_cancellation};

pub use notifications::safe_session_update;
pub use permission::{PermissionKind, PermissionRequest, PermissionResult};

/// A thin context object binding a session's cancellation channel for the
/// duration of a permission request, and exposing the connection needed to
/// send that request.
pub struct ToolExecutor<'a> {
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) cwd: &'a Path,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(
        cx: &'a ConnectionTo<Client>,
        session_id: &'a SessionId,
        cwd: &'a Path,
        cancellation: watch::Receiver<bool>,
    ) -> Self {
        bind_session_cancellation(
            session_id.0.as_ref(),
            ToolCancellation::from_receiver(cancellation),
        );
        Self {
            cx,
            session_id,
            cwd,
        }
    }
}

impl Drop for ToolExecutor<'_> {
    fn drop(&mut self) {
        unbind_session_cancellation(self.session_id.0.as_ref());
    }
}
