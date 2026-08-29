use serde_json::Value;

use super::super::sandbox::RiskLevel;

pub(crate) const MAX_DIFF_OLD_TEXT_BYTES: u64 = 64 * 1024;
pub(crate) const MAX_RAW_INPUT_CHARS: usize = 8 * 1024;
pub(crate) const MAX_RESULT_LOCATIONS: usize = 8;
pub(crate) const MAX_RESULT_PREVIEW_CHARS: usize = 4 * 1024;
pub(crate) const MAX_QUESTION_PREVIEW_CHARS: usize = 2 * 1024;
pub(crate) const MAX_CARD_BODY_CHARS: usize = 8 * 1024;

/// Host-neutral semantic tool presentation produced by `tool_ux`.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub title: String,
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

/// Host-neutral semantic representation of a tool result presentation.
#[derive(Debug, Clone)]
pub struct ResultUpdate {
    pub status: &'static str,
    pub content: Vec<Value>,
    pub locations: Vec<Value>,
}
