use super::TurnEventEmitter;
use crate::ToolUiModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnTermination {
    Cancelled,
    Failed,
}

pub trait TurnEventSink: Send {
    fn turn_started(&mut self) -> bool;
    fn ensure_turn_started(&mut self) -> bool;
    fn assistant_started(&mut self) -> bool;
    fn assistant_delta(&mut self, delta: String) -> bool;
    fn assistant_completed(&mut self) -> bool;
    fn thinking_started(&mut self) -> bool;
    fn thinking_delta(&mut self, delta: String) -> bool;
    fn thinking_completed(&mut self) -> bool;
    fn tool_call_requested(
        &mut self,
        upstream_id: String,
        name: String,
        ui: Option<ToolUiModel>,
    ) -> bool;
    fn permission_requested(&mut self, upstream_id: String) -> bool;
    fn tool_execution_started(&mut self, upstream_id: String, ui: Option<ToolUiModel>) -> bool;
    fn tool_result_received(
        &mut self,
        upstream_id: String,
        result: String,
        ui: Option<ToolUiModel>,
    ) -> bool;
    fn turn_terminated(&mut self, termination: TurnTermination) -> bool;
    fn turn_cancelled(&mut self) -> bool {
        self.turn_terminated(TurnTermination::Cancelled)
    }
    fn turn_failed(&mut self) -> bool {
        self.turn_terminated(TurnTermination::Failed)
    }
    fn turn_completed(&mut self) -> bool;
    fn is_terminal(&self) -> bool;
}

impl TurnEventSink for TurnEventEmitter {
    fn turn_started(&mut self) -> bool {
        TurnEventEmitter::turn_started(self)
    }
    fn ensure_turn_started(&mut self) -> bool {
        TurnEventEmitter::ensure_turn_started(self)
    }
    fn assistant_started(&mut self) -> bool {
        TurnEventEmitter::assistant_started(self)
    }
    fn assistant_delta(&mut self, delta: String) -> bool {
        TurnEventEmitter::assistant_delta(self, delta)
    }
    fn assistant_completed(&mut self) -> bool {
        TurnEventEmitter::assistant_completed(self)
    }
    fn thinking_started(&mut self) -> bool {
        TurnEventEmitter::thinking_started(self)
    }
    fn thinking_delta(&mut self, delta: String) -> bool {
        TurnEventEmitter::thinking_delta(self, delta)
    }
    fn thinking_completed(&mut self) -> bool {
        TurnEventEmitter::thinking_completed(self)
    }
    fn tool_call_requested(
        &mut self,
        upstream_id: String,
        name: String,
        ui: Option<ToolUiModel>,
    ) -> bool {
        TurnEventEmitter::tool_call_requested_with_ui(self, upstream_id, name, ui)
    }
    fn permission_requested(&mut self, upstream_id: String) -> bool {
        TurnEventEmitter::permission_requested(self, upstream_id)
    }
    fn tool_execution_started(&mut self, upstream_id: String, ui: Option<ToolUiModel>) -> bool {
        TurnEventEmitter::tool_execution_started_with_ui(self, upstream_id, ui)
    }
    fn tool_result_received(
        &mut self,
        upstream_id: String,
        result: String,
        ui: Option<ToolUiModel>,
    ) -> bool {
        TurnEventEmitter::tool_result_received_with_ui(self, upstream_id, result, ui)
    }
    fn turn_terminated(&mut self, termination: TurnTermination) -> bool {
        match termination {
            TurnTermination::Cancelled => TurnEventEmitter::turn_cancelled(self),
            TurnTermination::Failed => TurnEventEmitter::turn_failed(self),
        }
    }
    fn turn_completed(&mut self) -> bool {
        TurnEventEmitter::turn_completed(self)
    }
    fn is_terminal(&self) -> bool {
        TurnEventEmitter::is_terminal(self)
    }
}
