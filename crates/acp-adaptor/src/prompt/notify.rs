//! ACP notifications: messages, reasoning, tool UI and usage.
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Diff, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
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
) -> SessionNotification {
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(tool_call_from_ui(tool_call_id, ui)),
    )
}

fn tool_call_update_notification(
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> SessionNotification {
    let update = ToolCallUpdate::from(tool_call_from_ui(tool_call_id, ui));
    SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(update),
    )
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
    ui.content.iter().filter_map(project_rich_content).collect()
}

fn project_rich_content(value: &Value) -> Option<ToolCallContent> {
    match value.get("type").and_then(Value::as_str) {
        Some("text") | Some("content") => value
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| ToolCallContent::from(ContentBlock::Text(TextContent::new(text)))),
        Some("diff") => {
            let path = value.get("path")?.as_str()?.to_owned();
            let new_text = value.get("newText").and_then(Value::as_str).unwrap_or("");
            let old_text = value.get("oldText").and_then(Value::as_str).map(str::to_owned);
            Some(ToolCallContent::Diff(Diff::new(path, new_text.to_owned()).old_text(old_text)))
        }
        Some("terminal") => value
            .get("id")
            .and_then(Value::as_str)
            .map(|id| ToolCallContent::Terminal(agent_client_protocol::schema::v1::Terminal::new(id.to_owned()))),
        _ => None,
    }
}

fn rich_locations(ui: &ToolUiModel) -> Vec<ToolCallLocation> {
    ui.locations
        .iter()
        .filter_map(|value| {
            let path = value.get("path")?.as_str()?.to_owned();
            let location = ToolCallLocation::new(path);
            value
                .get("line")
                .and_then(Value::as_u64)
                .map(|line| location.line(line as u32))
                .or(Some(location))
        })
        .collect()
}

fn fallback_text_content(ui: &ToolUiModel) -> Vec<ToolCallContent> {
    ui.output
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
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
    cx.send_notification(tool_call_notification(session_id, tool_call_id, ui))
}

pub fn notify_tool_call_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    tool_call_id: &str,
    ui: &ToolUiModel,
) -> Result<(), AcpError> {
    cx.send_notification(tool_call_update_notification(session_id, tool_call_id, ui))
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
