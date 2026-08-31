//! Host-neutral semantic tool presentation builder.
//!
//! This module owns rich tool presentation semantics. It must not depend on ACP
//! presentation types; the ACP adaptor performs the protocol projection.

mod builders;
mod display;
mod results;
mod types;

#[cfg(test)]
mod tests;

pub use display::bounded_raw_input;
pub use results::{classify_risk, result_update};
pub use types::tool_ui_kind;
pub use types::{ResultUpdate, ToolInfo};
