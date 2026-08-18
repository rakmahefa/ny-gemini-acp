use super::emitter::TurnEventEmitter;
use crate::ToolEventSink;

impl ToolEventSink for TurnEventEmitter {
    fn tool_call_requested(&mut self, upstream_id: String, name: String) -> bool {
        TurnEventEmitter::tool_call_requested(self, upstream_id, name)
    }
    fn permission_requested(&mut self, upstream_id: String) -> bool {
        TurnEventEmitter::permission_requested(self, upstream_id)
    }
    fn tool_execution_started(&mut self, upstream_id: String) -> bool {
        TurnEventEmitter::tool_execution_started(self, upstream_id)
    }
    fn tool_result_received(&mut self, upstream_id: String, result: String) -> bool {
        TurnEventEmitter::tool_result_received(self, upstream_id, result)
    }
}
