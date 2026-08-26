//! Security sandbox module.
//!
//! The implementation remains behaviorally identical; this facade establishes
//! a stable module boundary for subsequent separation of path policy, command
//! analysis, and shell execution concerns.

mod implementation;

pub use implementation::{
    validate_path, RiskLevel, SecurityError, ShellAnalysis, ShellSandbox,
};
