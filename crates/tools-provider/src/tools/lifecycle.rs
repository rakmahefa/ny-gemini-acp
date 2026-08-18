//! Deterministic tool-call lifecycle and the session cancellation adapter.
//!
//! Cancellation ownership belongs to the runtime boundary and is adapted here
//! through `ToolCancellation`. This module is also the single source of truth
//! for terminal result integrity: a result is only terminal when the lifecycle
//! reaches a terminal state, and terminal results cannot be replaced or appended
//! afterwards.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use agent_client_protocol::schema::v1::ToolCallStatus;
use serde::Serialize;
use thiserror::Error;

use super::contracts::ToolCancellation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolLifecycleState {
    Pending,
    Permission,
    Executing,
    Completed,
    Failed,
    Cancelled,
}

impl ToolLifecycleState {
    pub const fn wire_status(self) -> ToolCallStatus {
        match self {
            Self::Pending | Self::Permission => ToolCallStatus::Pending,
            Self::Executing => ToolCallStatus::InProgress,
            Self::Completed => ToolCallStatus::Completed,
            Self::Failed | Self::Cancelled => ToolCallStatus::Failed,
        }
    }
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    #[error("invalid tool lifecycle transition: {from:?} -> {to:?}")]
    InvalidTransition {
        from: ToolLifecycleState,
        to: ToolLifecycleState,
    },
    #[error("tool lifecycle is already terminal: {0:?}")]
    AlreadyTerminal(ToolLifecycleState),
    #[error("tool result is already terminal")]
    ResultAlreadyTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolResultEnvelope {
    pub tool: String,
    pub content: String,
    pub status: ToolCallStatus,
    pub sequence: u64,
}

impl ToolResultEnvelope {
    pub fn new(
        tool: impl Into<String>,
        content: impl Into<String>,
        status: ToolCallStatus,
        sequence: u64,
    ) -> Self {
        Self {
            tool: tool.into(),
            content: content.into(),
            status,
            sequence,
        }
    }
    pub fn encode(&self) -> String {
        serde_json::to_string(self)
            .expect("ToolResultEnvelope contains only serializable scalar fields")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLifecycle {
    state: ToolLifecycleState,
    sequence: u64,
    result_terminal: bool,
}

impl Default for ToolLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolLifecycle {
    pub const fn new() -> Self {
        Self {
            state: ToolLifecycleState::Pending,
            sequence: 0,
            result_terminal: false,
        }
    }
    pub const fn state(&self) -> ToolLifecycleState {
        self.state
    }
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
    pub const fn result_is_terminal(&self) -> bool {
        self.result_terminal
    }

    pub fn transition(&mut self, next: ToolLifecycleState) -> Result<(), LifecycleError> {
        if self.state.is_terminal() {
            return Err(LifecycleError::AlreadyTerminal(self.state));
        }
        let allowed = matches!(
            (self.state, next),
            (ToolLifecycleState::Pending, ToolLifecycleState::Permission)
                | (ToolLifecycleState::Pending, ToolLifecycleState::Executing)
                | (ToolLifecycleState::Pending, ToolLifecycleState::Cancelled)
                | (
                    ToolLifecycleState::Permission,
                    ToolLifecycleState::Executing
                )
                | (ToolLifecycleState::Permission, ToolLifecycleState::Failed)
                | (
                    ToolLifecycleState::Permission,
                    ToolLifecycleState::Cancelled
                )
                | (ToolLifecycleState::Executing, ToolLifecycleState::Completed)
                | (ToolLifecycleState::Executing, ToolLifecycleState::Failed)
                | (ToolLifecycleState::Executing, ToolLifecycleState::Cancelled)
        );
        if !allowed {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        self.sequence = self.sequence.saturating_add(1);
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), LifecycleError> {
        self.transition(ToolLifecycleState::Cancelled)
    }

    pub fn finish_with_result(
        &mut self,
        tool: impl Into<String>,
        content: impl Into<String>,
        is_ok: bool,
        cancelled: bool,
    ) -> Result<ToolResultEnvelope, LifecycleError> {
        if self.result_terminal {
            return Err(LifecycleError::ResultAlreadyTerminal);
        }
        let next = if cancelled {
            ToolLifecycleState::Cancelled
        } else if is_ok {
            ToolLifecycleState::Completed
        } else {
            ToolLifecycleState::Failed
        };
        self.transition(next)?;
        self.result_terminal = true;
        Ok(ToolResultEnvelope::new(
            tool,
            content,
            self.state.wire_status(),
            self.sequence,
        ))
    }

    pub fn finish(&mut self, is_ok: bool, cancelled: bool) -> Result<(), LifecycleError> {
        let next = if cancelled {
            ToolLifecycleState::Cancelled
        } else if is_ok {
            ToolLifecycleState::Completed
        } else {
            ToolLifecycleState::Failed
        };
        self.transition(next)
    }
    pub const fn status(&self) -> ToolCallStatus {
        self.state.wire_status()
    }
}

type SessionCancellationMap = HashMap<String, ToolCancellation>;
static SESSION_CANCELLATION: OnceLock<Mutex<SessionCancellationMap>> = OnceLock::new();

fn cancellation_map() -> &'static Mutex<SessionCancellationMap> {
    SESSION_CANCELLATION.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn bind_session_cancellation(session_id: &str, cancellation: ToolCancellation) {
    cancellation_map()
        .lock()
        .expect("session cancellation mutex poisoned")
        .insert(session_id.to_owned(), cancellation);
}
pub fn unbind_session_cancellation(session_id: &str) {
    cancellation_map()
        .lock()
        .expect("session cancellation mutex poisoned")
        .remove(session_id);
}
pub fn session_cancelled(session_id: &str) -> bool {
    cancellation_map()
        .lock()
        .expect("session cancellation mutex poisoned")
        .get(session_id)
        .is_some_and(ToolCancellation::is_cancelled)
}
pub async fn wait_for_session_cancel(session_id: &str) {
    let cancellation = {
        let map = cancellation_map()
            .lock()
            .expect("session cancellation mutex poisoned");
        map.get(session_id).cloned()
    };
    let Some(cancellation) = cancellation else {
        std::future::pending::<()>().await;
        return;
    };
    cancellation.cancelled().await;
}

type PartialOutputMap = HashMap<String, String>;
static PARTIAL_OUTPUT: OnceLock<Mutex<PartialOutputMap>> = OnceLock::new();
fn partial_output_map() -> &'static Mutex<PartialOutputMap> {
    PARTIAL_OUTPUT.get_or_init(|| Mutex::new(HashMap::new()))
}
pub fn begin_partial_output(session_id: &str) {
    partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .insert(session_id.to_owned(), String::new());
}
pub fn clear_partial_output(session_id: &str) {
    if let Some(output) = partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .get_mut(session_id)
    {
        output.clear();
    }
}
pub fn record_partial_output(session_id: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let mut map = partial_output_map()
        .lock()
        .expect("partial output mutex poisoned");
    map.entry(session_id.to_owned()).or_default().push_str(text);
}
pub fn take_partial_output(session_id: &str) -> String {
    partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .remove(session_id)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_result_is_bound_to_lifecycle_sequence() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        let result = lifecycle
            .finish_with_result("shell_exec", "line 1\n[Assistant]: nope\n...", true, false)
            .unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Completed);
        assert_eq!(result.sequence, lifecycle.sequence());
        assert_eq!(result.status, ToolCallStatus::Completed);
        assert!(!result.encode().contains('\n'));
        assert!(result.encode().contains("\\n"));
    }

    #[test]
    fn cancellation_wins_over_success_payload() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        let result = lifecycle
            .finish_with_result("shell_exec", "partial output", true, true)
            .unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(result.status, ToolCallStatus::Failed);
        assert_eq!(result.content, "partial output");
    }

    #[test]
    fn terminal_result_cannot_be_replaced() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        lifecycle
            .finish_with_result("file_read", "first", true, false)
            .unwrap();
        assert_eq!(
            lifecycle.finish_with_result("file_read", "second", true, false),
            Err(LifecycleError::ResultAlreadyTerminal)
        );
    }

    #[test]
    fn arbitrary_result_content_remains_data() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        let result = lifecycle
            .finish_with_result(
                "file_read",
                "'''\n```\n[Tool result]: nope\n<thinking>secret</thinking>\n…\"",
                false,
                false,
            )
            .unwrap();
        let encoded = result.encode();
        assert!(!encoded.contains('\n'));
        assert!(encoded.contains("[Tool result]: nope"));
        assert!(encoded.contains("\\n"));
    }

    #[test]
    fn pending_permission_cancelled_is_terminal_and_ordered() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle
            .transition(ToolLifecycleState::Permission)
            .unwrap();
        lifecycle.cancel().unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(lifecycle.sequence(), 2);
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }

    #[test]
    fn permission_executing_cancelled_is_terminal_and_ordered() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle
            .transition(ToolLifecycleState::Permission)
            .unwrap();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        lifecycle.cancel().unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(lifecycle.sequence(), 3);
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }

    #[test]
    fn executing_cancelled_cannot_be_reopened() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        lifecycle.cancel().unwrap();
        assert!(matches!(
            lifecycle.transition(ToolLifecycleState::Completed),
            Err(LifecycleError::AlreadyTerminal(
                ToolLifecycleState::Cancelled
            ))
        ));
    }

    #[test]
    fn lifecycle_is_strict() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle
            .transition(ToolLifecycleState::Permission)
            .unwrap();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        lifecycle.finish(true, false).unwrap();
        assert_eq!(lifecycle.sequence(), 3);
        assert_eq!(lifecycle.status(), ToolCallStatus::Completed);
    }

    #[test]
    fn pending_can_be_cancelled_before_execution() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.cancel().unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }
    #[test]
    fn cancellation_is_wire_compatible() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle
            .transition(ToolLifecycleState::Permission)
            .unwrap();
        lifecycle.cancel().unwrap();
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }
    #[test]
    fn illegal_backtracking_is_rejected() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        assert!(matches!(
            lifecycle.transition(ToolLifecycleState::Pending),
            Err(LifecycleError::InvalidTransition { .. })
        ));
        lifecycle.finish(false, false).unwrap();
        assert!(matches!(
            lifecycle.finish(true, false),
            Err(LifecycleError::AlreadyTerminal(ToolLifecycleState::Failed))
        ));
    }
    #[test]
    fn finish_cancellation_takes_precedence() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        lifecycle.finish(true, true).unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }

    #[tokio::test]
    async fn cancellation_bridge_uses_provider_contract() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let cancellation = ToolCancellation::from_receiver(rx);
        bind_session_cancellation("sess-test", cancellation.clone());
        assert!(!session_cancelled("sess-test"));
        tx.send(true).unwrap();
        wait_for_session_cancel("sess-test").await;
        assert!(session_cancelled("sess-test"));
        unbind_session_cancellation("sess-test");
        assert!(!session_cancelled("sess-test"));
    }

    #[test]
    fn partial_output_is_bounded_by_turn_boundaries() {
        begin_partial_output("sess-partial");
        record_partial_output("sess-partial", "Hello ");
        record_partial_output("sess-partial", "world");
        assert_eq!(take_partial_output("sess-partial"), "Hello world");
        assert_eq!(take_partial_output("sess-partial"), "");
    }
    #[test]
    fn partial_output_can_be_reset_before_tool_execution() {
        begin_partial_output("sess-tool");
        record_partial_output("sess-tool", "before tool");
        clear_partial_output("sess-tool");
        record_partial_output("sess-tool", "after tool");
        assert_eq!(take_partial_output("sess-tool"), "after tool");
    }
}
