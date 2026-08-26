//! ACP-facing tool UX module.
//!
//! The implementation is exposed through this facade so the public tool UX
//! contract remains stable while its formatting, locations, and lifecycle
//! projections can be separated into focused submodules.

mod implementation;

pub use implementation::{ResultUpdate, ToolInfo};
