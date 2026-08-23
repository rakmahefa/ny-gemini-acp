//! Builtin tools.
//!
//! Filesystem, shell, search, web search, composed, and interactive-adjacent
//! tools all implement the same `Tool` trait and are registered through
//! `ToolRegistry`.

pub mod composed;
pub mod file;
pub mod filesystem;
pub mod search;
pub mod shell;
pub mod web_search;
