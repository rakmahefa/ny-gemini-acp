//! ACP notifications: messages, reasoning, tool UI and usage.
use std::path::PathBuf;

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Diff, MessageId, SessionId, SessionNotification, SessionUpdate,
    Terminal, TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolKind, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use agent_runtime::{ToolUiKind, ToolUiModel, ToolUiStatus};
use serde_json::Value;

use tools_provider::tools::lifecycle::record_partial_output;

pub const CONTEXT_TOKENS: u64 = 1_000_000;

pub fn usage_update(prompt: &str, assistant: &str) -> UsageUpdate {
    let used = (prompt.chars().count() + assistant.chars().count()) as u64 / 4;
    UsageUpdate::new(used, CONTEXT_TOKENS)
}

fn text_notification(
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                .message_id(message_id.clone()),
        ),
    )
}

fn reasoning_notification(
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentThoughtChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                .message_id(message_id.clone()),
        ),
    )
}

fn tool_call_notification(
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<SessionNotification, AcpError> {
    Ok(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(tool_call_from_ui(tool_call_id, ui)?),
    ))
}

fn tool_call_update_notification(
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<SessionNotification, AcpError> {
    let update = ToolCallUpdate::from(tool_call_from_ui(tool_call_id, ui)?);
    Ok(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(update),
    ))
}

fn usage_notification(
    session_id: &SessionId,
    prompt: &str,
    assistant: &str,
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::UsageUpdate(usage_update(prompt, assistant)),
    )
}

pub fn emit_error_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    error: &str,
) {
    cx.send_notification(text_notification(
        session_id,
        message_id,
        format!("\n\n[error] {error}"),
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
    cx.send_notification(text_notification(session_id, message_id, text))
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
    cx.send_notification(reasoning_notification(session_id, message_id, text))
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

fn tool_ui_meta(ui: &ToolUiModel) -> serde_json::Map<String, Value> {
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

fn projection_error() -> AcpError {
    AcpError::internal_error()
}

fn rich_content(ui: &ToolUiModel) -> Result<Vec<ToolCallContent>, AcpError> {
    ui.content.iter().map(project_content).collect()
}

fn project_content(value: &Value) -> Result<ToolCallContent, AcpError> {
    let kind = value.get("type").and_then(Value::as_str).ok_or_else(projection_error)?;
    match kind {
        "content" => {
            let text = value.get("text").and_then(Value::as_str).ok_or_else(projection_error)?;
            Ok(ToolCallContent::from(ContentBlock::Text(TextContent::new(text.to_owned()))))
        }
        "diff" => {
            let path = value.get("path").and_then(Value::as_str).ok_or_else(projection_error)?;
            let new_text = value.get("newText").and_then(Value::as_str).ok_or_else(projection_error)?;
            let old_text = value.get("oldText").and_then(Value::as_str).map(str::to_owned);
            Ok(ToolCallContent::Diff(
                Diff::new(PathBuf::from(path), new_text.to_owned()).old_text(old_text),
            ))
        }
        "terminal" => {
            let id = value.get("id").and_then(Value::as_str).ok_or_else(projection_error)?;
            Ok(ToolCallContent::Terminal(Terminal::new(id.to_owned())))
        }
        _ => Err(projection_error()),
    }
}

fn rich_locations(ui: &ToolUiModel) -> Result<Vec<ToolCallLocation>, AcpError> {
    ui.locations.iter().map(project_location).collect()
}

fn project_location(value: &Value) -> Result<ToolCallLocation, AcpError> {
    let path = value.get("path").and_then(Value::as_str).ok_or_else(projection_error)?;
    let location = ToolCallLocation::new(PathBuf::from(path));
    match value.get("line").and_then(Value::as_u64) {
        Some(line) => {
            let line = u32::try_from(line).map_err(|_| projection_error())?;
            Ok(location.line(line))
        }
        None => Ok(location),
    }
}

fn fallback_text_content(ui: &ToolUiModel) -> Vec<ToolCallContent> {
    ui.output
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| {
            vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
                text.to_owned(),
            )))]
        })
        .unwrap_or_default()
}

fn tool_call_from_ui(tool_call_id: &str, ui: &ToolUiModel) -> Result<ToolCall, AcpError> {
    let rich = rich_content(ui)?;
    let content = if rich.is_empty() {
        fallback_text_content(ui)
    } else {
        rich
    };
    let locations = rich_locations(ui)?;

    Ok(ToolCall::new(ToolCallId::from(tool_call_id.to_owned()), ui.title.clone())
        .kind(tool_kind(ui.kind))
        .status(tool_status(ui.status))
        .content(content)
        .locations(locations)
        .raw_input(ui.input.clone())
        .raw_output(ui.output.clone())
        .meta(tool_ui_meta(ui)))
}

pub fn notify_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    cx.send_notification(tool_call_notification(session_id, tool_call_id, ui)?)
}

pub fn notify_tool_call_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    cx.send_notification(tool_call_update_notification(session_id, tool_call_id, ui)?)
}

pub fn notify_usage(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    prompt: &str,
    assistant: &str,
) -> Result<(), AcpError> {
    cx.send_notification(usage_notification(session_id, prompt, assistant))
}

#[cfg(test)]
#[path = "../test/notify.rs"]
mod tests;
