//! ACP protocol adapter for the provider-neutral agent runtime.
//!
//! ACP-specific protocol and presentation code stays here; the underlying
//! runtime and provider implementations are imported through their canonical
//! crate boundaries.
pub mod config;

pub mod agent;
pub mod handlers;
pub mod prompt;
