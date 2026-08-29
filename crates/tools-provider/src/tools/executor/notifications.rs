use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionNotification, SessionUpdate, TextContent,
    ToolCall as AcpToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::ToolUiKind;
use serde_json::{Map, Value};

use super::super::lifecycle::ToolLifecycle;
use super::super::tool_ux::{bounded_raw_input, ToolInfo};
use super::{mapping, ToolExecutor};

pub(super) fn project_content(values: &[Value]) -> Vec<ToolCallContent> {
    values
        .iter()
        .filter_map(|value| match value.get("type").and_then(Value::as_str) {
            Some("text") | Some("content") => value
                .get("text")
                .and_then(Value::as_str)
                .map(|text| {
                    ToolCallContent::from(ContentBlock::Text(TextContent::new(text)))
                }),
            Some("diff") => {
                let path = value.get("path")?.as_str()?.to_owned();
                let new_text = value.get("newText").and_then(Value::as_str).unwrap_or("");
                let old_text = value
                    .get("oldText")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Some(ToolCallContent::Diff(
                    agent_client_protocol::schema::v1::Diff::new(
                        path,
                        new_text.to_owned(),
                    )
                    .old_text(old_text),
                ))
            }
            Some("terminal") => value
                .get("id")
                .and_then(Value::as_str)
                .map(|id| {
                    ToolCallContent::Terminal(
                        agent_client_protocol::schema::v1::Terminal::new(id.to_owned()),
                    )
                }),
            _ => None,
        })
        .collect()
}

pub(super) fn project_locations(values: &[Value]) -> Vec<ToolCallLocation> {
    values
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

pub(super) fn project_tool_kind(kind: ToolUiKind) -> ToolKind {
    match kind {
        ToolUiKind::FileRead | ToolUiKind::DirectoryList => ToolKind::Read,
        ToolUiKind::FileWrite | ToolUiKind::FileEdit | ToolUiKind::ReplaceInFile => {
            ToolKind::Edit
        }
        ToolUiKind::Search | ToolUiKind::Glob | ToolUiKind::SearchAndRead => ToolKind::Search,
        ToolUiKind::Shell => ToolKind::Execute,
        ToolUiKind::AskUserQuestion | ToolUiKind::Generic => ToolKind::Other,
    }
}

impl<'a> ToolExecutor<'a> {
    pub(super) fn emit_tool_call(
        &self,
        call_id: &ToolCallId,
        info: &ToolInfo,
        lifecycle: &ToolLifecycle,
        raw_input: &Value,
    ) {
        let tool = AcpToolCall::new(call_id.clone(), info.title.clone())
            .kind(project_tool_kind(info.kind))
            .status(lifecycle.state().wire_status())
            .content(project_content(&info.content))
            .locations(project_locations(&info.locations))
            .raw_input(bounded_raw_input(raw_input))
            .meta(mapping::lifecycle_meta(&info.title, lifecycle, None, None));
        let _ = self.cx.send_notification(SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::ToolCall(tool),
        ));
    }

    pub(super) fn emit_lifecycle(
        &self,
        call_id: &ToolCallId,
        lifecycle: &ToolLifecycle,
        tool_name: &str,
    ) {
        self.emit_update(
            call_id,
            lifecycle.state().wire_status(),
            vec![],
            vec![],
            Some(mapping::lifecycle_meta(tool_name, lifecycle, None, None)),
        );
    }

    pub(super) fn emit_update(
        &self,
        call_id: &ToolCallId,
        status: ToolCallStatus,
        content: Vec<ToolCallContent>,
        locations: Vec<ToolCallLocation>,
        meta: Option<Map<String, Value>>,
    ) {
        let update = ToolCallUpdate::new(
            call_id.clone(),
            ToolCallUpdateFields::new()
                .status(status)
                .content(content)
                .locations(locations),
        )
        .meta(meta);
        let _ = self.cx.send_notification(SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::ToolCallUpdate(update),
        ));
    }
}

pub fn safe_session_update(
    cx: &ConnectionTo<Client>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    update: SessionUpdate,
) {
    let _ = cx.send_notification(SessionNotification::new(session_id.clone(), update));
}

pub fn emit_error_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    message_id: &agent_client_protocol::schema::v1::MessageId,
    error: &str,
) {
    safe_session_update(
        cx,
        session_id,
        SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(format!(
                "\n\n[error] {error}"
            ))))
            .message_id(message_id.clone()),
        ),
    );
}
