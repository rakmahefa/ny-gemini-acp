use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionNotification, SessionUpdate, TextContent,
    ToolCall as AcpToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Client, ConnectionTo};
use serde_json::{Map, Value};

use super::super::lifecycle::ToolLifecycle;
use super::super::tool_ux::{bounded_raw_input, ToolInfo};
use super::{mapping, ToolExecutor};

impl<'a> ToolExecutor<'a> {
    pub(super) fn emit_tool_call(
        &self,
        call_id: &ToolCallId,
        info: &ToolInfo,
        lifecycle: &ToolLifecycle,
        raw_input: &Value,
    ) {
        let tool = AcpToolCall::new(call_id.clone(), info.title.clone())
            .kind(info.kind)
            .status(lifecycle.state().wire_status())
            .content(info.content.clone())
            .locations(info.locations.clone())
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
