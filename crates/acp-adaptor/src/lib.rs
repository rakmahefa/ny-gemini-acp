//! ACP protocol adapter for the provider-neutral agent runtime.
//!
//! The adapter keeps temporary compatibility aliases for the historical module
//! paths while exposing the new provider-neutral crates under their canonical
//! names. These aliases are local to the adapter and do not recreate the old
//! workspace crate boundaries.
extern crate self as gemini_acp_runtime;
extern crate self as gemini_acp_config;
extern crate self as gemini_acp_tools;

pub use agent_runtime::*;

/// Provider configuration surface used by ACP handlers.
pub mod config {
    pub use llm_provider::config::*;
}

/// Tool implementation surface used by ACP presentation/execution helpers.
pub mod tools {
    pub use tools_provider::tools::*;
}

pub mod agent;
pub mod elicitation;
pub mod handlers;
pub mod prompt;
pub mod thought;
pub mod utils;
