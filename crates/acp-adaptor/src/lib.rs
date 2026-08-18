//! ACP adapter: protocol transport and protocol-facing handlers.
//!
//! The adapter is deliberately thin. The domain runtime lives in
//! `agent-runtime`; model and tool implementations are providers.

// Temporary source-compatibility aliases during the workspace reset. The
// dependency itself is now the `agent-runtime` package.
extern crate gemini_acp_encaps as gemini_acp_runtime;

pub mod agent;
#[cfg(feature = "elicitation")]
pub mod elicitation;
pub mod handlers;
pub mod prompt;
pub mod thought;
pub mod utils;

pub use agent::run_agent;
pub use utils::{sleep, Pushable};
