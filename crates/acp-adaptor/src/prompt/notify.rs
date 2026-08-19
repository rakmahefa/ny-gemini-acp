//! ACP notifications : messages, reasoning, tool UI and usage.
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolKind,
    UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::{ToolUiKind, ToolUiModel, ToolUiStatus};

use tools_provider::tools::lifecycle::record_partial_output;

pub const CONTEXT_TOKENS: u64 = 1_000_000;

pub fn usage_update(prompt: &str, assistant: &str) -> UsageUpdate {
    let used = (prompt.chars().count() + assistant.chars().count()) as u64 / 4;
    UsageUpdate::new(used, CONTEXT_TOKENS)
}

/// Emits a non-fatal ACP error chunk from provider-facing turn orchestration.
pub fn emit_error_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    error: &str,
) {
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(format!(
                "\n\n[error] {error}"
            ))))
            .message_id(message_id.clone()),
        ),
    ))
    .ok();
}

/// ACP notification sink for already-normalized assistant text.
pub fn notify_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> Result<(), AcpError> {
    if text.is_empty() {
        return Ok(());
    }
    record_partial_output(session_id.0.as_ref(), &text);
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                .message_id(message_id.clone()),
        ),
    ))
}

/// ACP presentation of already-normalized model reasoning.
pub fn notify_reasoning(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> Result<(), AcpError> {
    if text.is_empty() {
        return Ok(());
    }
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentThoughtChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                .message_id(message_id.clone()),
        ),
    ))
}

fn tool_kind(kind: ToolUiKind) -> ToolKind {
    match kind {
        ToolUiKind::FileRead | ToolUiKind::DirectoryList => ToolKind::Read,
        ToolUiKind::FileWrite
        | ToolUiKind::FileEdit
        | ToolUiKind::ReplaceInFile => ToolKind::Edit,
        ToolUiKind::Search | ToolUiKind::Glob | ToolUiKind::SearchAndRead => ToolKind::Search,
        ToolUiKind::Shell => ToolKind::Execute,
        ToolUiKind::AskUserQuestion | ToolUiKind::Generic => ToolKind::Other,
    }
}

fn tool_status(status: ToolUiStatus) -> ToolCallStatus {
    match status {
        ToolUiStatus::Pending => ToolCallStatus::Pending,
        ToolUiStatus::Running => ToolCallStatus::InProgress,
        ToolUiStatus::Succeeded => ToolCallStatus::Completed,
        ToolUiStatus::Failed | ToolUiStatus::Cancelled => ToolCallStatus::Failed,
    }
}

fn tool_ui_meta(ui: &ToolUiModel) -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({
        "geminiAcp": {
            "toolUi": {
                "kind": format!("{:?}", ui.kind),
                "status": format!("{:?}", ui.status),
                "summary": ui.summary.clone(),
                "expandable": ui.expandable,
            }
        }
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

fn tool_call_from_ui(tool_call_id: &str, ui: &ToolUiModel) -> ToolCall {
    let content = ui
        .output
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(text)))])
        .unwrap_or_default();

    ToolCall::new(ToolCallId::from(tool_call_id.to_owned()), ui.title.clone())
        .kind(tool_kind(ui.kind))
        .status(tool_status(ui.status))
        .content(content)
        .raw_input(ui.input.clone())
        .raw_output(ui.output.clone())
        .meta(tool_ui_meta(ui))
}

/// Projects the semantic tool request into ACP's native tool-call lifecycle.
pub fn notify_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    let tool_call = tool_call_from_ui(tool_call_id, ui);
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(tool_call),
    ))
}

/// Projects a semantic tool lifecycle update into ACP's native ToolCallUpdate.
pub fn notify_tool_call_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    let update = ToolCallUpdate::from(tool_call_from_ui(tool_call_id, ui));
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(update),
    ))
}

pub fn notify_usage(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    prompt: &str,
    assistant: &str,
) -> Result<(), AcpError> {
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::UsageUpdate(usage_update(prompt, assistant)),
    ))
}

#[cfg(test)]
#[path = "../test/notify.rs"]
mod tests;
