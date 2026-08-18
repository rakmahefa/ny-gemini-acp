//! Provider-neutral tool implementations for the agent runtime.
//!
//! The historical tool modules are kept under `tools/` during the architecture
//! reset. This crate is the sole home for builtin and MCP tool implementations.

pub mod tools;
pub use tools::*;
