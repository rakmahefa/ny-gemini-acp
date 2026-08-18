//! Agent runtime: durable state, sessions, semantic events and execution.
//!
//! This crate is the center of the four-crate workspace architecture. ACP is
//! adapted outside this crate; model and tool implementations are providers.

pub mod events;
mod execution;
pub mod persona;
pub mod runtime;
pub mod session;
pub mod state;

// Provider facades used while the workspace reset is being completed. The
// implementations remain owned by `llm-provider`; these exports avoid forcing
// every adapter module to know the provider package's internal crate layout.
pub mod client {
    pub use gemini_acp_config::client::*;
}

pub mod config {
    pub use gemini_acp_config::config::*;
}

pub mod core {
    pub use gemini_acp_config::core::*;
}

pub use gemini_acp_tools as tools;

pub use execution::{
    AcpThread, AcpThreadHandle, AcpTurn, AcpTurnHandle, Cancellation, EncapsError,
    ThreadCommand, ThreadState, TurnManager, TurnState,
};
pub use events::{AcpSemanticEvent, EventBus, EventContext, EventStream, ToolEventContext};
pub use runtime::{AgentRuntime, AppState};
pub use session::SessionManager;
pub use tools::{ToolCallKind, ToolCallRequest, ToolCallRequestError, ToolCallState, ToolRegistry};
