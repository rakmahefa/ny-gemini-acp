use serde_json::Value;

use agent_runtime::{ToolUiKind, ToolUiStatus};

use super::super::sandbox::RiskLevel;

pub(crate) const MAX_DIFF_OLD_TEXT_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_RAW_INPUT_CHARS: usize = 8 * 1024;
pub(crate) const MAX_RESULT_LOCATIONS: usize = 8;
pub(crate) const MAX_RESULT_PREVIEW_CHARS: usize = 4 * 1024;
pub(crate) const MAX_QUESTION_PREVIEW_CHARS: usize = 2 * 1024;
pub(crate) const MAX_CARD_BODY_CHARS: usize = 8 * 1024;

/// C-30 : mapping unique nom d'outil → kind d'UI (fusion des deux copies
/// `ui_kind` (provider.rs) et `tool_ui_kind` (executor/mod.rs)).
pub fn tool_ui_kind(name: &str) -> ToolUiKind {
    match name {
        "file_read" => ToolUiKind::FileRead,
        "file_write" => ToolUiKind::FileWrite,
        "file_edit" => ToolUiKind::FileEdit,
        "glob" => ToolUiKind::Glob,
        "list_directory" => ToolUiKind::DirectoryList,
        "search" => ToolUiKind::Search,
        "search_and_read" => ToolUiKind::SearchAndRead,
        "shell_exec" => ToolUiKind::Shell,
        "replace_in_file" => ToolUiKind::ReplaceInFile,
        "AskUserQuestion" => ToolUiKind::AskUserQuestion,
        _ => ToolUiKind::Generic,
    }
}

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub title: String,
    pub kind: ToolUiKind,
    pub content: Vec<Value>,
    pub locations: Vec<Value>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CardBodyKind {
    Output,
    Content,
    Input,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolVisual {
    pub(crate) icon: &'static str,
    pub(crate) label: &'static str,
    pub(crate) permission: &'static str,
    pub(crate) risk: RiskLevel,
}

impl ToolVisual {
    pub(crate) fn for_tool(name: &str, args: &Value) -> Self {
        let (icon, label) = super::display::tool_visual(name);
        Self {
            icon,
            label,
            permission: super::display::permission_label(name),
            risk: super::results::classify_risk(name, args),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResultUpdate {
    pub status: ToolUiStatus,
    pub content: Vec<Value>,
    pub locations: Vec<Value>,
}
