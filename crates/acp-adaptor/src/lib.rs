//! ACP adapter: protocol transport and protocol-facing handlers.
//!
//! The adapter is deliberately thin. The domain runtime lives in
//! `agent-runtime`; model and tool implementations are providers.

// Internal source-compatibility bridge: this name now points at the
// `llm-provider` package and does not reintroduce the removed workspace crate.
extern crate gemini_acp_llm as gemini_acp_config;

pub mod agent;
pub mod config;
#[cfg(feature = "elicitation")]
pub mod elicitation;
pub mod handlers;
pub mod prompt;
pub mod thought;
pub mod utils;

pub use agent::run_agent;
pub use config::{build_agent_capabilities, build_config_options};
pub use utils::{sleep, Pushable};
