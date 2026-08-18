//! Agent runtime: durable state, sessions, semantic events and execution.
//!
//! This crate is the center of the four-crate workspace architecture. ACP is
//! adapted outside this crate; tools and the model integration are providers.

pub mod events;
mod execution;
pub mod persona;
pub mod runtime;
pub mod session;
pub mod state;

// Compatibility facade during the reset: the implementation now lives in
// `tools-provider`, but existing runtime/adapter code can keep using the
// `crate::tools` and `gemini_acp_runtime::tools` paths while the dependency
// boundary is enforced at the Cargo package level.
pub use gemini_acp_tools as tools;

pub use execution::{
    AcpThread, AcpThreadHandle, AcpTurn, AcpTurnHandle, Cancellation, EncapsError,
    ThreadCommand, ThreadState, TurnManager, TurnState,
};
pub use events::{AcpSemanticEvent, EventBus, EventContext, EventStream, ToolEventContext};
pub use runtime::{AgentRuntime, AppState};
pub use session::SessionManager;
pub use tools::{ToolCallKind, ToolCallRequest, ToolCallRequestError, ToolCallState, ToolRegistry};
