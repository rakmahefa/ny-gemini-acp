mod parser;
mod path;
mod risk;
mod shell;

#[cfg(test)]
mod attack_tests;
#[cfg(test)]
mod tests;

pub use parser::{parse_shell, ParsedShellCommand, ShellOperator, ShellParseError, ShellSegment};
pub use path::{validate_path, SecurityError};
pub use risk::{RiskLevel, ShellAnalysis};
pub use shell::ShellSandbox;
