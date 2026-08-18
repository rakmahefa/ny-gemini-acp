//! Agent runtime: durable state, sessions and execution.

pub mod events;
mod execution;
pub mod persona;
pub mod providers;
pub mod runtime;
pub mod session;
pub mod state;
pub mod time;

pub use events::{EventBus, EventContext, EventStream, SemanticEvent, ToolEventContext, TurnEventEmitter};
pub use execution::{
    AgentThread, AgentThreadHandle, AgentTurn, AgentTurnHandle, Cancellation, RuntimeError,
    ThreadCommand, ThreadState, TurnManager, TurnState,
};
pub use providers::{
    GenerationOptions, LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelEvent, ModelRequest,
    NullLlmProvider, NullToolProvider, SharedLlmProvider, SharedToolProvider, ToolCallRequest,
    ToolCallResult, ToolEventSink, ToolProvider, ToolServerConfig, ToolTransportKind,
};
pub use runtime::{AgentRuntime, AppState, RuntimeConfig};
pub use session::SessionManager;
