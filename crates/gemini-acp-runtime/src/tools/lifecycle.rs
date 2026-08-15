//! Deterministic tool-call lifecycle and the session cancellation adapter.
//!
//! Cancellation ownership remains in `gemini-acp-encaps::Cancellation`.
//! This module only adapts that primitive to the existing executor API; it
//! never creates a second cancellation source.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use agent_client_protocol::schema::v1::ToolCallStatus;
use gemini_acp_encaps::Cancellation;
use thiserror::Error;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLifecycle {
    state: ToolLifecycleState,
    sequence: u64,
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
        }
    }

    pub const fn state(&self) -> ToolLifecycleState {
        self.state
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
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
                | (ToolLifecycleState::Permission, ToolLifecycleState::Executing)
                | (ToolLifecycleState::Permission, ToolLifecycleState::Failed)
                | (ToolLifecycleState::Permission, ToolLifecycleState::Cancelled)
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

    /// Finalize an executing tool from the single lifecycle source of truth.
    ///
    /// Cancellation always wins over the success/error result when the caller
    /// has explicitly observed a session cancellation. This keeps the internal
    /// state and the ACP wire status consistent instead of duplicating the
    /// terminal-state decision in the executor.
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

    /// Return the protocol status represented by the current lifecycle state.
    pub const fn status(&self) -> ToolCallStatus {
        self.state.wire_status()
    }
}

type SessionCancellationMap = HashMap<String, Cancellation>;
static SESSION_CANCELLATION: OnceLock<Mutex<SessionCancellationMap>> = OnceLock::new();

fn cancellation_map() -> &'static Mutex<SessionCancellationMap> {
    SESSION_CANCELLATION.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register the current turn's `encaps` cancellation primitive for the legacy executor adapter.
pub fn bind_session_cancellation(session_id: &str, cancellation: Cancellation) {
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
        .is_some_and(Cancellation::is_cancelled)
}

/// Wait for the cancellation source owned by `encaps` to become cancelled.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_is_strict() {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Permission).unwrap();
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
        lifecycle
            .transition(ToolLifecycleState::Executing)
            .unwrap();
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
        lifecycle
            .transition(ToolLifecycleState::Executing)
            .unwrap();
        lifecycle.finish(true, true).unwrap();
        assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
        assert_eq!(lifecycle.status(), ToolCallStatus::Failed);
    }

    #[tokio::test]
    async fn cancellation_bridge_uses_encaps_source() {
        let cancellation = Cancellation::new();
        bind_session_cancellation("sess-test", cancellation.clone());
        assert!(!session_cancelled("sess-test"));
        cancellation.cancel();
        wait_for_session_cancel("sess-test").await;
        assert!(session_cancelled("sess-test"));
        unbind_session_cancellation("sess-test");
        assert!(!session_cancelled("sess-test"));
    }
}
