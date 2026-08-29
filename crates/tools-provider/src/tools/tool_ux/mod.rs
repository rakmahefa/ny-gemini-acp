//! Host-neutral tool UX builder.
//!
//! `tool_ux` owns rich semantic presentation for concrete tool invocations.
//! ACP protocol presentation is projected only at the ACP boundary.

mod builders;
mod display;
mod results;
mod types;

#[cfg(test)]
mod tests;

pub use display::bounded_raw_input;
pub use results::{classify_risk, lifecycle_icon, lifecycle_label, result_update};
pub use types::{ResultUpdate, ToolInfo};
