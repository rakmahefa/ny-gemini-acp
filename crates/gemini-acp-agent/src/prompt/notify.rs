//! Notifications ACP : chunks texte, usage tokens.
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};

use super::output_filter::sanitize_text;
use gemini_acp_runtime::tools::lifecycle::record_partial_output;

pub const CONTEXT_TOKENS: u64 = 1_000_000;

pub fn usage_update(prompt: &str, assistant: &str) -> UsageUpdate {
    let used = (prompt.chars().count() + assistant.chars().count()) as u64 / 4;
    UsageUpdate::new(used, CONTEXT_TOKENS)
}

/// Frontière unique entre le texte Gemini et `AgentMessageChunk`.
/// Les marqueurs ACP échappés sont supprimés avant toute notification.
pub fn notify_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> Result<(), AcpError> {
    let text = sanitize_text(&text);
    if text.is_empty() {
        return Ok(());
    }
    // The client has now seen this exact filtered text. Keep a turn-local copy
    // so `session/cancel` can persist already-visible partial output even when
    // the normal `total_output` finalization path is intentionally skipped.
    record_partial_output(session_id.0.as_ref(), &text);
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                .message_id(message_id.clone()),
        ),
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
