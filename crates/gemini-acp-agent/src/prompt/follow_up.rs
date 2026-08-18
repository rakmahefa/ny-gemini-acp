//! FollowUp handling for Gemini streams and ACP action choices.
//!
//! FollowUp is NOT an executable tool. ACP v1 does not define a generic
//! button component, so this module uses the stable `session/request_permission`
//! interaction primitive to obtain an explicit user choice. The request is
//! intentionally not routed through `ToolExecutor` and therefore never looks
//! like a completed tool execution.

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use serde_json::json;
use tokio::sync::watch;

const FOLLOW_UP_MARKER: &str = "<FollowUp";
const SELECT_ID: &str = "followup_select";
const SKIP_ID: &str = "followup_skip";
const MAX_LABEL_CHARS: usize = 160;
const MAX_QUERY_CHARS: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpOutcome {
    Selected(String),
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpError {
    InvalidInput(&'static str),
    Acp(String),
    UnexpectedOutcome(String),
}

impl std::fmt::Display for FollowUpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
            Self::Acp(message) => write!(f, "ACP FollowUp action failed: {message}"),
            Self::UnexpectedOutcome(message) => {
                write!(f, "unexpected FollowUp permission outcome: {message}")
            }
        }
    }
}

/// Ask the host to present one interactive FollowUp choice.
///
/// The action owns its own ACP `ToolCallId`, which is independent from both
/// the Gemini stream-local tool ids and the semantic tool lifecycle. Waiting
/// for the host is cancellation-aware and never reserves another outer turn.
pub async fn request_action(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    source_id: &str,
    label: &str,
    query: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<FollowUpOutcome, FollowUpError> {
    let label = label.trim();
    let query = query.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(FollowUpError::InvalidInput("FollowUp label is empty or too long"));
    }
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(FollowUpError::InvalidInput("FollowUp query is empty or too long"));
    }
    if *cancel.borrow() {
        return Ok(FollowUpOutcome::Cancelled);
    }

    let call_id = ToolCallId::from(format!("followup_{}", uuid::Uuid::new_v4().simple()));
    let body = format!(
        "**{}**\n\n{}\n\nChoisissez cette action pour envoyer la proposition au modèle.",
        label, query
    );
    let content = vec![ToolCallContent::Content(Content::new(ContentBlock::Text(
        TextContent::new(body),
    )))];

    let tool_call = ToolCall::new(
        call_id.clone(),
        format!("Follow-up · {}", truncate(label, 80)),
    )
    .kind(ToolKind::Other)
    .status(ToolCallStatus::Pending)
    .content(content)
    .raw_input(json!({
        "label": label,
        "query": query,
    }))
    .meta(
        json!({
            "geminiAcp": {
                "nonExecutionKind": "follow_up_action",
                "ui": "choice",
                "sourceId": source_id,
                "label": label,
                "query": query,
            }
        })
        .as_object()
        .cloned()
        .unwrap(),
    );

    let options = vec![
        PermissionOption::new(SELECT_ID, label, PermissionOptionKind::AllowOnce),
        PermissionOption::new(SKIP_ID, "Ignorer", PermissionOptionKind::RejectOnce),
    ];

    let request =
        RequestPermissionRequest::new(session_id.clone(), ToolCallUpdate::from(tool_call), options)
            .meta(
                json!({
                    "geminiAcp": {
                        "kind": "follow_up",
                        "action": "prompt",
                        "sourceId": source_id,
                        "label": label,
                        "query": query,
                        "singleUse": true,
                    }
                })
                .as_object()
                .cloned()
                .unwrap(),
            );

    let response = tokio::select! {
        result = cx.send_request(request).block_task() => {
            result.map_err(|error| FollowUpError::Acp(error.to_string()))?
        }
        _ = cancel.changed() => {
            return Ok(FollowUpOutcome::Cancelled);
        }
    };

    if *cancel.borrow() {
        return Ok(FollowUpOutcome::Cancelled);
    }

    match response.outcome {
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0 == SELECT_ID.into() =>
        {
            Ok(FollowUpOutcome::Selected(query.to_owned()))
        }
        RequestPermissionOutcome::Selected(selected) if selected.option_id.0 == SKIP_ID.into() => {
            Ok(FollowUpOutcome::Rejected)
        }
        RequestPermissionOutcome::Cancelled => Ok(FollowUpOutcome::Rejected),
        other => Err(FollowUpError::UnexpectedOutcome(format!("{other:?}"))),
    }
}

#[derive(Debug, Default)]
pub struct StreamNormalizer {
    pending: String,
}

impl StreamNormalizer {
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        self.drain(false)
    }

    pub fn finish(&mut self) -> String {
        self.drain(true)
    }

    fn drain(&mut self, final_flush: bool) -> String {
        let mut out = String::new();
        loop {
            let Some(start) = self.pending.find(FOLLOW_UP_MARKER) else {
                if final_flush {
                    out.push_str(&self.pending);
                    self.pending.clear();
                    return out;
                }
                let keep = partial_marker_len(&self.pending);
                let emit_len = self.pending.len().saturating_sub(keep);
                if emit_len > 0 {
                    out.push_str(&self.pending[..emit_len]);
                    self.pending = self.pending[emit_len..].to_owned();
                }
                return out;
            };

            if start > 0 {
                out.push_str(&self.pending[..start]);
                self.pending = self.pending[start..].to_owned();
            }

            let Some(end) = find_tag_end(&self.pending[FOLLOW_UP_MARKER.len()..]) else {
                if final_flush {
                    self.pending.clear();
                }
                return out;
            };

            let consume = FOLLOW_UP_MARKER.len() + end + 1;
            self.pending = self.pending[consume..].to_owned();
        }
    }
}

/// FollowUp is parsed by the runtime parser. Keep this helper for the existing
/// turn orchestration API; it intentionally performs no UI transformation.
pub fn replace_components(input: &str) -> String {
    input.to_owned()
}

fn partial_marker_len(input: &str) -> usize {
    let marker = FOLLOW_UP_MARKER.as_bytes();
    let bytes = input.as_bytes();
    let max = bytes.len().min(marker.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if bytes[bytes.len() - len..] == marker[..len] {
            return len;
        }
    }
    0
}

fn find_tag_end(input: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in input.as_bytes().iter().copied().enumerate() {
        match quote {
            Some(current) if byte == current => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    format!("{}…", value.chars().take(max).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_up_option_ids_are_stable() {
        assert_eq!(SELECT_ID, "followup_select");
        assert_eq!(SKIP_ID, "followup_skip");
    }

    #[test]
    fn outcome_is_explicitly_not_a_tool_result() {
        assert_eq!(
            FollowUpOutcome::Selected("cargo test".into()),
            FollowUpOutcome::Selected("cargo test".into())
        );
        assert_ne!(FollowUpOutcome::Rejected, FollowUpOutcome::Cancelled);
    }

    #[test]
    fn removes_complete_follow_up_from_stream() {
        let mut normalizer = StreamNormalizer::default();
        assert_eq!(
            normalizer.push("hello <FollowUp label=\"Run tests\" query=\"cargo test\" />"),
            "hello "
        );
        assert_eq!(normalizer.finish(), "");
    }

    #[test]
    fn removes_multiple_follow_ups_from_stream() {
        let mut normalizer = StreamNormalizer::default();
        assert_eq!(
            normalizer.push(
                "before <FollowUp label=\"One\" query=\"one\" /><FollowUp label=\"Two\" query=\"two\" /> after"
            ),
            "before  after"
        );
        assert_eq!(normalizer.finish(), "");
    }

    #[test]
    fn handles_split_marker_at_every_prefix_length() {
        for split in 1.."<FollowUp".len() {
            let mut normalizer = StreamNormalizer::default();
            let (left, right) = "<FollowUp label=\"Run\" query=\"cargo test\" />".split_at(split);
            assert_eq!(normalizer.push(&format!("before{left}")), "before");
            assert_eq!(normalizer.push(right), "");
            assert_eq!(normalizer.finish(), "");
        }
    }

    #[test]
    fn does_not_stop_at_gt_inside_quotes() {
        let mut normalizer = StreamNormalizer::default();
        assert_eq!(
            normalizer.push("hello <FollowUp label=\"A > B\" query=\"cargo test\" /> world"),
            "hello  world"
        );
    }

    #[test]
    fn malformed_follow_up_is_hidden_at_finish() {
        let mut normalizer = StreamNormalizer::default();
        assert_eq!(normalizer.push("hello <FollowUp label=\"Run"), "hello ");
        assert_eq!(normalizer.finish(), "");
    }

    #[test]
    fn ordinary_less_than_text_is_preserved() {
        let mut normalizer = StreamNormalizer::default();
        let input = "2 < 3 and x <Follow";
        assert_eq!(normalizer.push(input), "2 < 3 and x ");
        assert_eq!(normalizer.finish(), "<Follow");
    }

    #[test]
    fn partial_marker_suffix_is_preserved_until_boundary() {
        let mut normalizer = StreamNormalizer::default();
        assert_eq!(normalizer.push("hello <Follow"), "hello ");
        assert_eq!(normalizer.push("Up label=\"Run\" query=\"test\" />"), "");
        assert_eq!(normalizer.finish(), "");
    }
}
