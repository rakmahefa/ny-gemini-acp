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
    SessionNotification::new(session_id.clone(), SessionUpdate::ToolCallUpdate(update))
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

pub fn notify_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> Result<(), AcpError> {
    if text.is_empty() {
        return Ok(());
    }
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

/// I-10 : identifiants stables exposés au client (le `{:?}` de Debug Rust
/// était un format non contractuel, cassable par un simple renommage).
fn tool_ui_kind_id(kind: ToolUiKind) -> &'static str {
    match kind {
        ToolUiKind::FileRead => "file_read",
        ToolUiKind::FileWrite => "file_write",
        ToolUiKind::FileEdit => "file_edit",
        ToolUiKind::ReplaceInFile => "replace_in_file",
        ToolUiKind::DirectoryList => "directory_list",
        ToolUiKind::Search => "search",
        ToolUiKind::Glob => "glob",
        ToolUiKind::SearchAndRead => "search_and_read",
        ToolUiKind::Shell => "shell",
        ToolUiKind::AskUserQuestion => "ask_user_question",
        ToolUiKind::Generic => "generic",
    }
}

fn tool_ui_status_id(status: ToolUiStatus) -> &'static str {
    match status {
        ToolUiStatus::Pending => "pending",
        ToolUiStatus::Running => "running",
        ToolUiStatus::Succeeded => "succeeded",
        ToolUiStatus::Failed => "failed",
        ToolUiStatus::Cancelled => "cancelled",
    }
}

fn tool_ui_meta(ui: &ToolUiModel) -> serde_json::Map<String, Value> {
    serde_json::json!({
        "geminiAcp": {
            "toolUi": {
                "kind": tool_ui_kind_id(ui.kind),
                "status": tool_ui_status_id(ui.status),
                "summary": ui.summary.clone(),
                "expandable": ui.expandable,
            }
        }
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// D-07 : projection tolérante — un item `ui.content` inconnu ou malformé est
/// ignoré (avec un warn) au lieu de tuer tout le turn par `internal_error`.
/// Un item bien formé est projeté, les autres tombent sur
/// `fallback_text_content` si rien ne reste.
fn project_content(value: &Value) -> Option<ToolCallContent> {
    let warn_skip = |reason: &str| {
        tracing::warn!(
            item = %value,
            reason,
            "ui.content item ignored by the ACP projection (text fallback if nothing remains)"
        );
    };
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        warn_skip("missing type");
        return None;
    };
    let projected = match kind {
        "content" => value
            .get("text")
            .and_then(Value::as_str)
            .map(|text| ToolCallContent::from(ContentBlock::Text(TextContent::new(text.to_owned())))),
        "diff" => {
            let path = value.get("path").and_then(Value::as_str);
            let new_text = value.get("newText").and_then(Value::as_str);
            match (path, new_text) {
                (Some(path), Some(new_text)) => {
                    let old_text = value.get("oldText").and_then(Value::as_str).map(str::to_owned);
                    Some(ToolCallContent::Diff(
                        Diff::new(PathBuf::from(path), new_text.to_owned()).old_text(old_text),
                    ))
                }
                _ => None,
            }
        }
        "terminal" => value
            .get("id")
            .and_then(Value::as_str)
            .map(|id| ToolCallContent::Terminal(Terminal::new(id.to_owned()))),
        _ => None,
    };
    if projected.is_none() {
        warn_skip("malformed or unknown item");
    }
    projected
}

fn rich_content(ui: &ToolUiModel) -> Vec<ToolCallContent> {
    ui.content.iter().filter_map(project_content).collect()
}

fn rich_locations(ui: &ToolUiModel) -> Vec<ToolCallLocation> {
    ui.locations.iter().filter_map(project_location).collect()
}

fn project_location(value: &Value) -> Option<ToolCallLocation> {
    let path = value.get("path").and_then(Value::as_str)?;
    let location = ToolCallLocation::new(PathBuf::from(path));
    match value.get("line").and_then(Value::as_u64) {
        Some(line) => {
            let line = u32::try_from(line).ok()?;
            Some(location.line(line))
        }
        None => Some(location),
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

fn tool_call_from_ui(tool_call_id: &str, ui: &ToolUiModel) -> ToolCall {
    let rich = rich_content(ui);
    let content = if rich.is_empty() {
        fallback_text_content(ui)
    } else {
        rich
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
