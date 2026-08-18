//! ACP protocol adapter for the provider-neutral agent runtime.
//!
//! ACP-specific protocol and presentation code stays here; the underlying
//! runtime and provider implementations are imported through their canonical
//! crate boundaries.
pub use agent_runtime::*;

pub mod config;

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
