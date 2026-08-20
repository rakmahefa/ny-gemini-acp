use std::sync::Arc;

use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::{Cancellation, LlmProvider, ToolProvider};
use agent_runtime::state::Store;

pub struct TurnContext<'a> {
    pub store: Arc<Store>,
    pub tools: Arc<dyn ToolProvider>,
    pub llm: Arc<dyn LlmProvider>,
    pub cx: ConnectionTo<Client>,
    pub semantic: &'a mut TurnEventEmitter,
    pub cancellation: Cancellation,
}
