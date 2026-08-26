//! Agent runtime: durable state, sessions and execution.

pub mod events;
mod execution;
pub mod identity;
pub mod persona;
pub mod prompt;
pub mod providers;
pub mod runtime;
pub mod session;
pub mod state;
pub mod text;
pub mod time;
pub mod tool_ui;

pub use events::{
    EventBus, EventContext, EventStream, SemanticEvent, ToolEventContext, TurnEventEmitter,
    TurnEventSink, TurnTermination,
};
pub use execution::{
    AgentActionHandler, AgentLoop, AgentLoopConfig, AgentLoopError, AgentLoopOutcome,
    AgentTurn, AgentTurnHandle, Cancellation, RuntimeError, ToolPermissionDecision,
    ToolPermissionHandler, ToolPermissionRequest, TurnExecutionResult, TurnManager, TurnService,
    TurnServiceError, TurnState,
};
pub use identity::{SessionId, ToolCallId, TurnId};
pub use prompt::{
    format_tool_call, format_tool_result, TOOL_CALL_CLOSE, TOOL_CALL_OPEN, TOOL_RESULT_PREFIX,
};
pub use providers::{
    GenerationOptions, LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelEvent, ModelRequest,
    NullLlmProvider, NullToolProvider, SharedLlmProvider, SharedToolProvider, ToolCallRequest,
    ToolCallResult, ToolProvider, ToolServerConfig, ToolTransportKind,
};
pub use runtime::{AgentRuntime, AppState, RuntimeConfig};
pub use session::SessionManager;
pub use tool_ui::{ToolUiKind, ToolUiModel, ToolUiStatus};
