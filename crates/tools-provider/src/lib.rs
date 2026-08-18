//! Provider-neutral tool implementations for the agent runtime.
//!
//! This crate is intentionally independent from `agent-runtime`. The historical
//! modules remain grouped below `tools/`, but runtime-facing contracts are
//! defined locally and consumed through explicit provider traits.

extern crate self as gemini_acp_encaps;

pub mod tools;

pub use tools::*;

/// Compatibility name for legacy lifecycle internals. The concrete cancellation
/// primitive is now owned by the tools provider instead of the old encaps crate.
pub type Cancellation = ToolCancellation;
