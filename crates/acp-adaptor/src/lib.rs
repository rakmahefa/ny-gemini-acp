//! ACP adapter: protocol transport and protocol-facing handlers.
//!
//! The adapter is deliberately thin. The domain runtime lives in
//! `agent-runtime`; model and tool implementations are providers.

pub mod agent;
#[cfg(feature = "elicitation")]
pub mod elicitation;
pub mod handlers;
pub mod prompt;
pub mod thought;
pub mod utils;

pub use agent::run_agent;
pub use utils::{sleep, Pushable};
