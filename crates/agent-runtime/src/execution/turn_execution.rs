use std::sync::Arc;

use crate::events::TurnEventSink;
use crate::state::Session;
use crate::{AgentActionHandler, Cancellation, ToolPermissionHandler, ToolProvider};

pub type TurnPromptBuilder = fn(&Session, &dyn ToolProvider) -> String;

/// Inputs required to execute one already-acquired turn.
pub struct TurnExecutionRequest<'a> {
    pub session_id: String,
    pub session: Session,
    pub generation: u64,
    pub references: Vec<String>,
    pub cancellation: Cancellation,
    pub semantic: &'a mut dyn TurnEventSink,
    pub action_handler: Option<Arc<dyn AgentActionHandler>>,
    pub permission_handler: Option<Arc<dyn ToolPermissionHandler>>,
    pub build_prompt: TurnPromptBuilder,
}
