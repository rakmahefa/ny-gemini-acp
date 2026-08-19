//! Agent runtime: durable state, sessions and execution.

pub mod events;
mod execution;
pub mod persona;
pub mod providers;
pub mod runtime;
pub mod session;
pub mod state;
pub mod time;
pub mod tool_ui;

pub use events::{EventBus, EventContext, EventStream, SemanticEvent, ToolEventContext, TurnEventEmitter};
pub use execution::{
    AgentActionHandler, AgentLoop, AgentLoopConfig, AgentLoopError, AgentLoopOutcome,
    AgentThread, AgentThreadHandle, AgentTurn, AgentTurnHandle, Cancellation, RuntimeError,
    ThreadCommand, ThreadState, ToolPermissionDecision, ToolPermissionHandler,
    ToolPermissionRequest, TurnManager, TurnState,
};
pub use providers::{
    GenerationOptions, LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelEvent, ModelRequest,
    NullLlmProvider, NullToolProvider, SharedLlmProvider, SharedToolProvider, ToolCallRequest,
    ToolCallResult, ToolEventSink, ToolProvider, ToolServerConfig, ToolTransportKind,
};
pub use runtime::{AgentRuntime, AppState, RuntimeConfig};
pub use session::SessionManager;
pub use tool_ui::{ToolUiKind, ToolUiModel, ToolUiStatus};
