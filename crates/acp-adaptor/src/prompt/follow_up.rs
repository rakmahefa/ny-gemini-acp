//! FollowUp handling for ACP action choices.
//!
//! FollowUp is NOT an executable tool. ACP v1 does not define a generic
//! button component, so this module uses the stable `session/request_permission`
//! interaction primitive to obtain an explicit user choice. The request is
//! intentionally not routed through `ToolExecutor` and therefore never looks
//! like a completed tool execution.
//!
//! C-21 : la moitié morte de ce module (StreamNormalizer, parsing
//! `<FollowUp>` dupliqué du runtime, replace_components no-op, truncate) a été
//! supprimée — le parsing du flux est assuré par le runtime
//! (`tools_provider::tools::parse`), seul `request_action` vit ici.

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use serde_json::json;
use tokio::sync::watch;

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
        return Err(FollowUpError::InvalidInput(
            "FollowUp label is empty or too long",
        ));
    }
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return Err(FollowUpError::InvalidInput(
            "FollowUp query is empty or too long",
        ));
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
        format!("Follow-up · {}", agent_runtime::text::truncate_chars(label, 80)),
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
        // D-16 : `Cancelled` est une variante de `FollowUpOutcome` déjà traitée
        // par l'action (→ AgentActionError::Cancelled) — la convertir en
        // `Rejected` transformait une annulation en refus utilisateur.
        RequestPermissionOutcome::Cancelled => Ok(FollowUpOutcome::Cancelled),
        other => Err(FollowUpError::UnexpectedOutcome(format!("{other:?}"))),
    }
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
}
