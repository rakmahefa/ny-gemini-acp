//! ACP protocol adapter for the provider-neutral agent runtime.
extern crate self as gemini_acp_runtime;
extern crate self as gemini_acp_config;
extern crate self as gemini_acp_tools;

pub use agent_runtime::*;
pub mod config { pub use llm_provider::config::*; }
pub mod tools { pub use tools_provider::tools::*; }

pub mod agent;
pub mod elicitation;
pub mod handlers;
pub mod prompt;
pub mod thought;
pub mod utils;
