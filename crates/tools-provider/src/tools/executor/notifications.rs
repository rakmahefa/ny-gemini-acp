use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent,
};
use agent_client_protocol::{Client, ConnectionTo};

pub fn safe_session_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    update: SessionUpdate,
) {
    let _ = cx.send_notification(SessionNotification::new(session_id.clone(), update));
}

pub fn emit_error_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
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
