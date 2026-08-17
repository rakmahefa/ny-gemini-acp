use agent_client_protocol::schema::v1::{
    ContentBlock, SessionId, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_runtime::state::Role;

use gemini_acp_runtime::tools::executor::safe_session_update;

pub(crate) const CONTEXT_WINDOW_CHARS: usize = 1_000_000;
pub(crate) const COMPACTION_THRESHOLD_CHARS: usize = (CONTEXT_WINDOW_CHARS as f64 * 0.9) as usize;
pub(crate) const EMERGENCY_COMPACTION_CHARS: usize = (CONTEXT_WINDOW_CHARS as f64 * 0.7) as usize;
const PRESERVE_TURNS: usize = 10;

pub(crate) fn compact_messages(messages: &mut Vec<(Role, String)>, target_chars: usize) {
    if messages.len() <= 1 {
        return;
    }

    let mut turns = Vec::new();
    let mut current = Vec::new();
    for message in messages.iter() {
        if message.0 == Role::User && !current.is_empty() {
            turns.push(std::mem::take(&mut current));
        }
        current.push(message.clone());
    }
    if !current.is_empty() {
        turns.push(current);
    }

    if turns.len() <= PRESERVE_TURNS {
        return;
    }

    let current_chars: usize = messages.iter().map(|(_, text)| text.len()).sum();
    if current_chars <= target_chars {
        return;
    }

    let tail_end = turns.len().saturating_sub(PRESERVE_TURNS);
    let mut candidates: Vec<(usize, usize)> = (0..tail_end)
        .map(|index| {
            (
                index,
                turns[index]
                    .iter()
                    .map(|(_, text)| text.len())
                    .sum::<usize>(),
            )
        })
        .collect();
    candidates.sort_by_key(|item| std::cmp::Reverse(item.1));

    let mut to_evict = std::collections::HashSet::new();
    let mut remaining_chars = current_chars;
    for (index, turn_chars) in candidates {
        if remaining_chars <= target_chars {
            break;
        }
        to_evict.insert(index);
        remaining_chars -= turn_chars;
    }

    let mut compacted = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        if index < tail_end && to_evict.contains(&index) {
            continue;
        }
        compacted.extend(turn.iter().cloned());
    }
    *messages = compacted;
}

pub(crate) async fn upload_images(
    client: &gemini_acp_config::client::Client,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    images: &[(String, String)],
) -> Result<Vec<String>, ()> {
    let mut refs = Vec::new();
    if images.is_empty() {
        return Ok(refs);
    }

    let total = images.len();
    let upload_call_id = ToolCallId::from(format!("call_{}", uuid::Uuid::new_v4().simple()));
    safe_session_update(
        cx,
        session_id,
        SessionUpdate::ToolCall(
            ToolCall::new(upload_call_id.clone(), format!("Upload {total} image(s) (Scotty)"))
                .kind(ToolKind::Fetch)
                .status(ToolCallStatus::InProgress),
        ),
    );

    for (index, (base64, mime)) in images.iter().enumerate() {
        match client.upload_image(base64, mime).await {
            Ok(reference) => refs.push(reference),
            Err(error) => {
                let content = vec![ToolCallContent::Content(
                    agent_client_protocol::schema::v1::Content::new(ContentBlock::Text(
                        TextContent::new(format!(
                            "Upload image {}/{} échoué: {error:#}",
                            index + 1,
                            total
                        )),
                    )),
                )];
                safe_session_update(
                    cx,
                    session_id,
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        upload_call_id.clone(),
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Failed)
                            .content(content),
                    )),
                );
                return Err(());
            }
        }
    }

    let content = vec![ToolCallContent::Content(
        agent_client_protocol::schema::v1::Content::new(ContentBlock::Text(TextContent::new(
            format!("{total} image(s) uploadée(s) avec succès"),
        ))),
    )];
    safe_session_update(
        cx,
        session_id,
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            upload_call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .content(content),
        )),
    );

    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_preserves_the_latest_ten_turns() {
        let mut messages = Vec::new();
        for index in 0..12 {
            messages.push((Role::User, format!("user-{index}")));
            messages.push((Role::Assistant, format!("assistant-{index}")));
        }

        compact_messages(&mut messages, 1);

        assert_eq!(messages.len(), 20);
        assert!(messages.iter().all(|(_, text)| {
            text.ends_with("-2")
                || text.ends_with("-3")
                || text.ends_with("-4")
                || text.ends_with("-5")
                || text.ends_with("-6")
                || text.ends_with("-7")
                || text.ends_with("-8")
                || text.ends_with("-9")
                || text.ends_with("-10")
                || text.ends_with("-11")
        }));
    }
}
