//! ACP-facing tool UX module.
//!
//! Responsibilities are split into presentation types, card rendering,
//! tool builders, and result/location projections.

mod builders;
mod display;
mod results;
mod types;

#[cfg(test)]
mod tests;

pub use display::bounded_raw_input;
pub use results::{classify_risk, lifecycle_icon, lifecycle_label, result_update};
pub use types::{ResultUpdate, ToolInfo};
