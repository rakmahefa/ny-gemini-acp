//! State machines of a turn, and their boundaries (C-32):
//!
//! - `TurnPhase` / `ToolPhase` (this module): the **integrity** view. They
//!   model what the turn pipeline is *allowed* to emit — an append-only
//!   journal of semantic events validated by `check_transition`. They are
//!   private to the integrity layer and never cross the crate boundary.
//! - `ToolUiStatus` / `ToolUiKind` (`agent-runtime::tool_ui`): the
//!   **presentation** view, a loss-free projection of lifecycle/phase states
//!   for host UI cards. It carries no transition logic.
//!
//! Correspondence: a pending or permission-bound tool call projects to
//! `ToolUiStatus::Pending`, a running call to `Running`, completion to
//! `Succeeded`, failure and cancellation to `Failed`; the integrity
//! `ToolPhase` tracks the same progress from the emitted-events side and
//! must never be *ahead* of the presented state.
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPhase {
    NotStarted,
    Active,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamPhase {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolTerminalReason {
    Result,
    PermissionDenied,
    TurnCancelled,
    TurnFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPhase {
    Requested,
    Permission,
    Executing,
    Terminal(ToolTerminalReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IntegrityError {
    pub(super) message: String,
}

impl IntegrityError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TurnIntegrity {
    phase: TurnPhase,
    thinking: StreamPhase,
    assistant: StreamPhase,
    tools: HashMap<String, ToolPhase>,
    work_started: bool,
}

impl Default for TurnIntegrity {
    fn default() -> Self {
        Self {
            phase: TurnPhase::NotStarted,
            thinking: StreamPhase::Idle,
            assistant: StreamPhase::Idle,
            tools: HashMap::new(),
            work_started: false,
        }
    }
}

impl TurnIntegrity {
    pub(super) fn phase(&self) -> TurnPhase {
        self.phase
    }
    pub(super) fn assistant_active(&self) -> bool {
        self.assistant == StreamPhase::Active
    }
    pub(super) fn thinking_active(&self) -> bool {
        self.thinking == StreamPhase::Active
    }
    pub(super) fn open_tool_ids(&self) -> Vec<String> {
        self.tools
            .iter()
            .filter_map(|(id, state)| {
                (!matches!(state, ToolPhase::Terminal(_))).then_some(id.clone())
            })
            .collect()
    }

    pub(super) fn turn_started(&mut self) -> Result<(), IntegrityError> {
        if self.phase != TurnPhase::NotStarted {
            return Err(IntegrityError::new(
                "turn_started must be the first turn event",
            ));
        }
        self.phase = TurnPhase::Active;
        Ok(())
    }

    pub(super) fn assistant_started(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_started")?;
        if self.assistant == StreamPhase::Active {
            return Err(IntegrityError::new("assistant stream is already active"));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "assistant cannot start while thinking is active",
            ));
        }
        self.assistant = StreamPhase::Active;
        self.work_started = true;
        Ok(())
    }

    pub(super) fn assistant_delta(&self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_delta")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new(
                "assistant_delta requires an active assistant stream",
            ));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "assistant_delta cannot be emitted while thinking is active",
            ));
        }
        Ok(())
    }

    pub(super) fn assistant_completed(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_completed")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new(
                "assistant_completed requires an active assistant stream",
            ));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "thinking must complete before assistant completes",
            ));
        }
        self.assistant = StreamPhase::Idle;
        Ok(())
    }

    pub(super) fn assistant_yields_to_action(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_yields_to_action")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new(
                "assistant_yields_to_action requires an active assistant stream",
            ));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "thinking must complete before assistant yields to an action",
            ));
        }
        self.assistant = StreamPhase::Idle;
        Ok(())
    }

    pub(super) fn thinking_started(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("thinking_started")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new(
                "thinking requires an active assistant stream",
            ));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new("thinking stream is already active"));
        }
        self.thinking = StreamPhase::Active;
        Ok(())
    }

    pub(super) fn thinking_delta(&self) -> Result<(), IntegrityError> {
        self.ensure_active("thinking_delta")?;
        if self.thinking != StreamPhase::Active {
            return Err(IntegrityError::new(
                "thinking_delta requires an active thinking stream",
            ));
        }
        Ok(())
    }

    pub(super) fn thinking_completed(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("thinking_completed")?;
        if self.thinking != StreamPhase::Active {
            return Err(IntegrityError::new(
                "thinking_completed requires an active thinking stream",
            ));
        }
        self.thinking = StreamPhase::Idle;
        Ok(())
    }

    pub(super) fn tool_call_requested(&mut self, id: &str) -> Result<(), IntegrityError> {
        self.ensure_active("tool_call_requested")?;
        if id.is_empty() {
            return Err(IntegrityError::new(
                "tool_call_requested requires a non-empty tool_call_id",
            ));
        }
        if self.assistant == StreamPhase::Active || self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "tool_call_requested requires text streams to be closed",
            ));
        }
        if self.tools.contains_key(id) {
            return Err(IntegrityError::new(format!(
                "tool call {id} was already requested"
            )));
        }
        self.tools.insert(id.to_owned(), ToolPhase::Requested);
        self.work_started = true;
        Ok(())
    }

    pub(super) fn permission_requested(&mut self, id: &str) -> Result<(), IntegrityError> {
        self.ensure_active("permission_requested")?;
        match self.tools.get_mut(id) {
            Some(s @ ToolPhase::Requested) => {
                *s = ToolPhase::Permission;
                Ok(())
            }
            Some(s) => Err(IntegrityError::new(format!(
                "permission_requested for tool {id} is invalid from state {s:?}"
            ))),
            None => Err(IntegrityError::new(format!(
                "permission_requested references unknown tool {id}"
            ))),
        }
    }

    pub(super) fn tool_execution_started(&mut self, id: &str) -> Result<(), IntegrityError> {
        self.ensure_active("tool_execution_started")?;
        match self.tools.get_mut(id) {
            Some(s @ (ToolPhase::Requested | ToolPhase::Permission)) => {
                *s = ToolPhase::Executing;
                Ok(())
            }
            Some(s) => Err(IntegrityError::new(format!(
                "tool_execution_started for tool {id} is invalid from state {s:?}"
            ))),
            None => Err(IntegrityError::new(format!(
                "tool_execution_started references unknown tool {id}"
            ))),
        }
    }

    pub(super) fn tool_result_received(&mut self, id: &str) -> Result<(), IntegrityError> {
        self.ensure_active("tool_result_received")?;
        match self.tools.get_mut(id) {
            Some(s @ ToolPhase::Executing) => {
                *s = ToolPhase::Terminal(ToolTerminalReason::Result);
                Ok(())
            }
            Some(s @ ToolPhase::Permission) => {
                *s = ToolPhase::Terminal(ToolTerminalReason::PermissionDenied);
                Ok(())
            }
            Some(ToolPhase::Requested) => Err(IntegrityError::new(format!(
                "tool_result_received for tool {id} requires execution or an explicit permission decision"
            ))),
            Some(s) => Err(IntegrityError::new(format!(
                "tool_result_received for tool {id} is invalid from state {s:?}"
            ))),
            None => Err(IntegrityError::new(format!(
                "tool_result_received references unknown tool {id}"
            ))),
        }
    }

    pub(super) fn abort_open_tools(
        &mut self,
        reason: ToolTerminalReason,
    ) -> Result<(), IntegrityError> {
        self.ensure_active("abort_open_tools")?;
        if matches!(
            reason,
            ToolTerminalReason::Result | ToolTerminalReason::PermissionDenied
        ) {
            return Err(IntegrityError::new(
                "abort_open_tools requires a turn cancellation or failure reason",
            ));
        }
        for state in self.tools.values_mut() {
            if !matches!(state, ToolPhase::Terminal(_)) {
                *state = ToolPhase::Terminal(reason);
            }
        }
        Ok(())
    }

    pub(super) fn terminal_reason_for(&self, event: &str) -> ToolTerminalReason {
        match event {
            "turn_cancelled" => ToolTerminalReason::TurnCancelled,
            "turn_failed" => ToolTerminalReason::TurnFailed,
            _ => ToolTerminalReason::TurnFailed,
        }
    }

    pub(super) fn finish_terminal_after_scopes(
        &mut self,
        event: &str,
    ) -> Result<(), IntegrityError> {
        self.ensure_active(event)?;
        if event == "turn_completed" && !self.work_started {
            return Err(IntegrityError::new(format!(
                "{event} requires at least one assistant or tool lifecycle event"
            )));
        }
        if self.assistant_active() || self.thinking_active() || !self.open_tool_ids().is_empty() {
            return Err(IntegrityError::new(format!(
                "{event} requires all semantic scopes to be closed before the terminal event"
            )));
        }
        self.phase = TurnPhase::Terminal;
        Ok(())
    }

    fn ensure_active(&self, event: &str) -> Result<(), IntegrityError> {
        if self.phase != TurnPhase::Active {
            return Err(IntegrityError::new(format!(
                "{event} requires an active turn, current state is {:?}",
                self.phase
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.assistant_started().unwrap();
        s.thinking_started().unwrap();
        assert!(s.assistant_completed().is_err());
        s.thinking_delta().unwrap();
        s.thinking_completed().unwrap();
        s.assistant_delta().unwrap();
        s.assistant_completed().unwrap();
        s.finish_terminal_after_scopes("turn_completed").unwrap();
        assert_eq!(s.phase, TurnPhase::Terminal);
    }

    #[test]
    fn assistant_can_yield_without_declaring_turn_completion() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.assistant_started().unwrap();
        s.assistant_delta().unwrap();
        s.assistant_yields_to_action().unwrap();
        assert!(!s.assistant_active());
        s.tool_call_requested("c").unwrap();
        assert!(!s.open_tool_ids().is_empty());
    }

    #[test]
    fn tool_lifecycle_requires_real_execution_for_results() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        assert!(s.tool_result_received("c").is_err());
        s.tool_execution_started("c").unwrap();
        s.tool_result_received("c").unwrap();
        assert!(matches!(
            s.tools.get("c"),
            Some(ToolPhase::Terminal(ToolTerminalReason::Result))
        ));
        s.finish_terminal_after_scopes("turn_completed").unwrap();
    }

    #[test]
    fn permission_result_is_an_explicit_terminal_outcome() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        s.permission_requested("c").unwrap();
        s.tool_result_received("c").unwrap();
        assert!(matches!(
            s.tools.get("c"),
            Some(ToolPhase::Terminal(ToolTerminalReason::PermissionDenied))
        ));
        assert!(s.tool_execution_started("c").is_err());
        s.finish_terminal_after_scopes("turn_completed").unwrap();
    }

    #[test]
    fn cancelled_tool_is_aborted_without_a_result_event() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        s.abort_open_tools(ToolTerminalReason::TurnCancelled)
            .unwrap();
        s.finish_terminal_after_scopes("turn_cancelled").unwrap();
        assert!(s.open_tool_ids().is_empty());
        assert_eq!(s.phase, TurnPhase::Terminal);
    }

    #[test]
    fn cancellation_can_terminalize_immediately_after_turn_start() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.finish_terminal_after_scopes("turn_cancelled").unwrap();
        assert_eq!(s.phase, TurnPhase::Terminal);
    }

    #[test]
    fn failure_can_terminalize_immediately_after_turn_start() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.finish_terminal_after_scopes("turn_failed").unwrap();
        assert_eq!(s.phase, TurnPhase::Terminal);
    }

    #[test]
    fn completion_still_rejects_open_tools() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        assert!(s.finish_terminal_after_scopes("turn_completed").is_err());
    }
}
