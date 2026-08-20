use serde_json::Value;

pub(super) const REASONING_OPEN_MARKERS: [&str; 4] = ["<thinking>", "<think>", "[Thinking]:", "[thinking]:"];
pub(super) const REASONING_CLOSE_MARKERS: [&str; 2] = ["</thinking>", "</think>"];
pub(super) const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
pub(super) const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
pub(super) const TOOL_RESULT_INLINE: &str = "[tool_result ";
pub(super) const TOOL_CALL_INLINE: &str = "[tool_call ";
pub(super) const TOOL_CALL_FENCE: &str = "```tool_call";
pub(super) const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";
pub(super) const FUNCTION_CALL_FENCE: &str = "```function_call";
pub(super) const FOLLOW_UP_PREFIX: &str = "<FollowUp";
pub(super) const MAX_PENDING: usize = 256 * 1024;
pub(super) const MAX_FOLLOW_UP: usize = 64 * 1024;
pub(super) const MAX_TOOL_BLOCK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReasoningPhase {
    Detecting,
    Response,
    Reasoning,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockKind {
    Tool,
    Function,
    SingleQuoteTool,
}

impl BlockKind {
    pub(super) fn opening(self) -> &'static str {
        match self {
            Self::Tool => TOOL_CALL_FENCE,
            Self::Function => FUNCTION_CALL_FENCE,
            Self::SingleQuoteTool => TOOL_CALL_SINGLE_QUOTE_FENCE,
        }
    }
    pub(super) fn closing(self) -> &'static str {
        match self {
            Self::SingleQuoteTool => "'''",
            Self::Tool | Self::Function => "```",
        }
    }
}

#[derive(Debug)]
pub(super) enum ProtocolMode {
    Normal,
    IgnoreToolResult { closing: Option<&'static str> },
    IgnoreInlineToolResult,
    ToolBlock { kind: BlockKind, body: String, oversized: bool },
    InlineToolCall,
}

#[derive(Debug)]
pub(super) enum ProtocolEvent {
    Text(String),
    ToolCall(ModelToolCall),
}

#[derive(Debug)]
pub(super) struct ModelToolCall {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: Value,
}
