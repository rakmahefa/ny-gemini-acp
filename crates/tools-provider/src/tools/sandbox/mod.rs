//! Security sandbox module.
//!
//! The sandbox is split by responsibility: filesystem scope, risk analysis,
//! and shell command policy.

mod path;
mod risk;
mod shell;

#[cfg(test)]
mod tests;

pub use path::{validate_path, SecurityError};
pub use risk::{RiskLevel, ShellAnalysis};
pub use shell::ShellSandbox;
