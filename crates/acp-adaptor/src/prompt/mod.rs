mod action;
pub mod build;
pub mod content;
pub mod error;
pub mod follow_up;
pub mod handler;
pub mod notify;
pub mod title;
pub mod tool_stream;
pub mod turn;
pub mod turn_context;

/// Backward-compatible stream module name; implementation now projects
/// ToolUiModel through native ACP tool-call updates.
pub use tool_stream as stream;

pub use handler::handle_prompt;
pub use turn::run_turn;
pub use turn_context::TurnContext;
