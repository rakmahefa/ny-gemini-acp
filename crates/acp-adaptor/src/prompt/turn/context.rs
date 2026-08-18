use agent_client_protocol::schema::v1::{
    ContentBlock, SessionId, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_runtime::state::Role;
use gemini_acp_runtime::LlmProvider;
use gemini_acp_tools::tools::executor::safe_session_update;
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
        .map(|i| (i, turns[i].iter().map(|(_, t)| t.len()).sum::<usize>()))
        .collect();
    candidates.sort_by_key(|item| std::cmp::Reverse(item.1));
    let mut to_evict = std::collections::HashSet::new();
    let mut remaining = current_chars;
    for (i, chars) in candidates {
        if remaining <= target_chars {
            break;
        }
        to_evict.insert(i);
        remaining -= chars;
    }
    let mut compacted = Vec::new();
    for (i, turn) in turns.iter().enumerate() {
        if i < tail_end && to_evict.contains(&i) {
            continue;
        }
        compacted.extend(turn.iter().cloned());
    }
    *messages = compacted;
}
pub(crate) async fn upload_images(
    llm: &dyn LlmProvider,
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
            ToolCall::new(upload_call_id.clone(), format!("Upload {total} image(s)"))
                .kind(ToolKind::Fetch)
                .status(ToolCallStatus::InProgress),
        ),
    );
    for (index, (base64, mime)) in images.iter().enumerate() {
        match llm.upload_image(base64, mime).await {
            Ok(reference) => refs.push(reference),
            Err(error) => {
                let content = vec![ToolCallContent::Content(
                    agent_client_protocol::schema::v1::Content::new(ContentBlock::Text(
                        TextContent::new(format!(
                            "Upload image {}/{} échoué: {error}",
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
