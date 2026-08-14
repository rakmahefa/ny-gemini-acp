//! Notifications ACP : chunks texte, usage tokens.
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent, UsageUpdate,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};

pub const CONTEXT_TOKENS: u64 = 1_000_000;

pub fn usage_update(prompt: &str, assistant: &str) -> UsageUpdate {
    let used = (prompt.chars().count() + assistant.chars().count()) as u64 / 4;
    UsageUpdate::new(used, CONTEXT_TOKENS)
}

/// Keeps ACP protocol markers out of the user-visible assistant stream.
/// Gemini can occasionally echo the textual history envelope after a tool round.
fn sanitize_visible_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("[Tool result for ") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("[Assistant]:") {
            if !out.is_empty() { out.push('\n'); }
            out.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("[User]:") {
            if !out.is_empty() { out.push('\n'); }
            out.push_str(rest.trim_start());
            continue;
        }
        if !out.is_empty() { out.push('\n'); }
        out.push_str(line);
    }
    out
}

pub fn notify_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: String,
) -> Result<(), AcpError> {
    let text = sanitize_visible_text(&text);
    if text.is_empty() {
        return Ok(());
    }
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
mod tests {
    use super::sanitize_visible_text;

    #[test]
    fn hides_tool_result_envelope() {
        assert_eq!(sanitize_visible_text("[Tool result for shell_exec]: Finished `dev` profile"), "");
    }

    #[test]
    fn strips_assistant_role_marker() {
        assert_eq!(sanitize_visible_text("[Assistant]: J'exécute cargo check"), "J'exécute cargo check");
    }

    #[test]
    fn preserves_normal_text() {
        assert_eq!(sanitize_visible_text("Compilation terminée."), "Compilation terminée.");
    }
}
