//! ACP notifications: messages, reasoning, tool UI and usage.
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolKind, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::{ToolUiKind, ToolUiModel, ToolUiStatus};

use tools_provider::tools::lifecycle::record_partial_output;

pub const CONTEXT_TOKENS: u64 = 1_000_000;

pub fn usage_update(prompt: &str, assistant: &str) -> UsageUpdate {
    let used = (prompt.chars().count() + assistant.chars().count()) as u64 / 4;
    UsageUpdate::new(used, CONTEXT_TOKENS)
}

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
        ToolUiKind::FileWrite | ToolUiKind::FileEdit | ToolUiKind::ReplaceInFile => ToolKind::Edit,
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

fn rich_content(ui: &ToolUiModel) -> Vec<ToolCallContent> {
    ui.content
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

fn rich_locations(ui: &ToolUiModel) -> Vec<ToolCallLocation> {
    ui.locations
        .iter()
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

fn fallback_text_content(ui: &ToolUiModel) -> Vec<ToolCallContent> {
    ui.output
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| {
            vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
                text,
            )))]
        })
        .unwrap_or_default()
}

fn tool_call_from_ui(tool_call_id: &str, ui: &ToolUiModel) -> ToolCall {
    let content = {
        let rich = rich_content(ui);
        if rich.is_empty() {
            fallback_text_content(ui)
        } else {
            rich
        }
    };
    let locations = rich_locations(ui);

    ToolCall::new(ToolCallId::from(tool_call_id.to_owned()), ui.title.clone())
        .kind(tool_kind(ui.kind))
        .status(tool_status(ui.status))
        .content(content)
        .locations(locations)
        .raw_input(ui.input.clone())
        .raw_output(ui.output.clone())
        .meta(tool_ui_meta(ui))
}

pub fn notify_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(tool_call_from_ui(tool_call_id, ui)),
    ))
}

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
