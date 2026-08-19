mod action;
pub mod build;
pub mod content;
pub mod error;
pub mod follow_up;
pub mod notify;
pub mod title;
pub mod tool_stream;
pub mod turn;

/// Backward-compatible stream module name; implementation now projects
/// ToolUiModel through native ACP tool-call updates.
pub use tool_stream as stream;

pub use turn::run_turn;
