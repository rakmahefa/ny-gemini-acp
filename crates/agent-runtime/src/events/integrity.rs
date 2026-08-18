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
enum ToolPhase {
    Requested,
    Permission,
    Executing,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IntegrityError {
    pub(super) message: String,
}

impl IntegrityError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
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

    pub(super) fn turn_started(&mut self) -> Result<(), IntegrityError> {
        if self.phase != TurnPhase::NotStarted {
            return Err(IntegrityError::new("turn_started must be the first turn event"));
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
            return Err(IntegrityError::new("assistant cannot start while thinking is active"));
        }
        self.assistant = StreamPhase::Active;
        self.work_started = true;
        Ok(())
    }

    pub(super) fn assistant_delta(&self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_delta")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new("assistant_delta requires an active assistant stream"));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new("assistant_delta cannot be emitted while thinking is active"));
        }
        Ok(())
    }

    pub(super) fn assistant_completed(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("assistant_completed")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new("assistant_completed requires an active assistant stream"));
        }
        if self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new("thinking must complete before assistant completes"));
        }
        self.assistant = StreamPhase::Idle;
        Ok(())
    }

    pub(super) fn thinking_started(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("thinking_started")?;
        if self.assistant != StreamPhase::Active {
            return Err(IntegrityError::new("thinking requires an active assistant stream"));
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
            return Err(IntegrityError::new("thinking_delta requires an active thinking stream"));
        }
        Ok(())
    }

    pub(super) fn thinking_completed(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("thinking_completed")?;
        if self.thinking != StreamPhase::Active {
            return Err(IntegrityError::new("thinking_completed requires an active thinking stream"));
        }
        self.thinking = StreamPhase::Idle;
        Ok(())
    }

    pub(super) fn tool_call_requested(&mut self, id: &str) -> Result<(), IntegrityError> {
        self.ensure_active("tool_call_requested")?;
        if id.is_empty() {
            return Err(IntegrityError::new("tool_call_requested requires a non-empty tool_call_id"));
        }
        if self.tools.contains_key(id) {
            return Err(IntegrityError::new(format!("tool call {id} was already requested")));
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
            Some(s @ (ToolPhase::Requested | ToolPhase::Permission | ToolPhase::Executing)) => {
                *s = ToolPhase::Terminal;
                Ok(())
            }
            Some(s) => Err(IntegrityError::new(format!(
                "tool_result_received for tool {id} is invalid from state {s:?}"
            ))),
            None => Err(IntegrityError::new(format!(
                "tool_result_received references unknown tool {id}"
            ))),
        }
    }

    pub(super) fn turn_cancelled(&mut self) -> Result<(), IntegrityError> {
        self.finish_terminal("turn_cancelled")
    }

    pub(super) fn turn_failed(&mut self) -> Result<(), IntegrityError> {
        self.finish_terminal("turn_failed")
    }

    pub(super) fn turn_completed(&mut self) -> Result<(), IntegrityError> {
        self.ensure_active("turn_completed")?;
        if !self.work_started {
            return Err(IntegrityError::new(
                "turn_completed requires at least one assistant or tool lifecycle event",
            ));
        }
        if self.assistant == StreamPhase::Active || self.thinking == StreamPhase::Active {
            return Err(IntegrityError::new(
                "turn_completed cannot close an active text stream",
            ));
        }
        if self.tools.values().any(|s| *s != ToolPhase::Terminal) {
            return Err(IntegrityError::new(
                "turn_completed cannot close a turn with an active tool call",
            ));
        }
        self.phase = TurnPhase::Terminal;
        Ok(())
    }

    fn finish_terminal(&mut self, event: &str) -> Result<(), IntegrityError> {
        self.ensure_active(event)?;
        self.phase = TurnPhase::Terminal;
        self.assistant = StreamPhase::Idle;
        self.thinking = StreamPhase::Idle;
        for s in self.tools.values_mut() {
            *s = ToolPhase::Terminal;
        }
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
        assert!(s.turn_completed().is_err());
        s.turn_started().unwrap();
        s.assistant_started().unwrap();
        s.thinking_started().unwrap();
        assert!(s.assistant_completed().is_err());
        s.thinking_delta().unwrap();
        s.thinking_completed().unwrap();
        s.assistant_delta().unwrap();
        s.assistant_completed().unwrap();
        s.turn_completed().unwrap();
        assert_eq!(s.phase, TurnPhase::Terminal);
    }

    #[test]
    fn tools() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        s.permission_requested("c").unwrap();
        s.tool_result_received("c").unwrap();
        s.turn_completed().unwrap();
    }

    #[test]
    fn cancel() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.tool_call_requested("c").unwrap();
        s.turn_cancelled().unwrap();
        assert!(s.turn_completed().is_err());
        assert!(s.turn_cancelled().is_err());
    }

    #[test]
    fn failure_is_terminal_even_with_open_lifecycle() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        s.assistant_started().unwrap();
        s.thinking_started().unwrap();
        s.tool_call_requested("c").unwrap();
        s.turn_failed().unwrap();
        assert!(s.turn_completed().is_err());
        assert!(s.turn_failed().is_err());
        assert!(s.assistant_delta().is_err());
        assert!(s.thinking_delta().is_err());
    }

    #[test]
    fn failed_start_cannot_be_completed() {
        let mut s = TurnIntegrity::default();
        s.turn_started().unwrap();
        assert!(s.turn_completed().is_err());
    }
}
