//! Host-neutral tool UX builder.
//!
//! `tool_ux` owns rich semantic presentation for concrete tool invocations.
//! It must not construct or import ACP presentation types. The ACP adaptor is
//! the only layer that projects `ToolUiModel` into ACP-native content.

mod builders;
mod display;
mod results;
mod types;

#[cfg(test)]
mod tests;

pub use display::bounded_raw_input;
pub use results::{classify_risk, lifecycle_icon, lifecycle_label, result_update};
pub use types::{ResultUpdate, ToolInfo};
