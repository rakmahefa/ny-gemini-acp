//! Gemini-provider configuration and option parsing.
//!
//! Configuration belongs to the provider boundary; the agent runtime receives
//! already-normalized runtime configuration and provider traits.
pub mod config_options;
pub mod env;

pub use config_options::*;
pub use env::*;
