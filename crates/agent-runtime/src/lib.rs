//! Agent runtime: durable state, sessions, events and execution.

pub mod events;
mod execution;
pub mod persona;
pub mod providers;
pub mod runtime;
pub mod session;
pub mod state;
pub mod time;

pub use events::{AcpSemanticEvent, EventBus, EventContext, EventStream, ToolEventContext};
pub use execution::{
    AcpThread, AcpThreadHandle, AcpTurn, AcpTurnHandle, Cancellation, EncapsError, ThreadCommand,
    ThreadState, TurnManager, TurnState,
};
pub use providers::{
    LlmModelInfo, LlmProvider, LlmRequest, LlmStream, NullLlmProvider, NullToolProvider,
    SharedLlmProvider, SharedToolProvider, ToolCallRequest, ToolCallResult, ToolEventSink,
    ToolProvider,
};
pub use runtime::{AgentRuntime, AppState, RuntimeConfig};
pub use session::SessionManager;
