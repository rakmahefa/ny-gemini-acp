// C-36 : `context.rs` renommé `uploads.rs` — ce module gère l'upload des
// images, pas le contexte du tour (éviter la confusion avec `turn_context.rs`).
mod permission;
mod uploads;

use super::action;
use super::content::blocks_to_parts;
use super::notify::notify_usage;
use super::title::derive_title;
use super::turn_context::TurnContext;
use agent_client_protocol::schema::v1::{
    PromptRequest, PromptResponse, SessionInfoUpdate, SessionUpdate, StopReason,
};
use agent_client_protocol::Error as AcpError;
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::state::{Role, TurnError};
use agent_runtime::{
    AgentLoopError, LlmError, LlmProviderErrorKind, SessionManager, TurnExecutionRequest,
};
use permission::AcpToolPermissionHandler;
use tools_provider::tools::executor::safe_session_update;

fn fail_before_execution(semantic: &mut TurnEventEmitter) {
    if semantic.is_terminal() {
        return;
    }
    let _ = semantic.turn_started();
    let _ = semantic.turn_failed();
}

fn map_agent_error(error: &AgentLoopError) -> Option<StopReason> {
    match error {
        AgentLoopError::Cancelled => Some(StopReason::Cancelled),
        // D-10 : la limite de rounds d'outils n'est pas une saturation de
        // tokens — la sémantique ACP fidèle est MaxTurnRequests.
        AgentLoopError::MaxRounds(_) => Some(StopReason::MaxTurnRequests),
        _ => None,
    }
}

fn agent_error_kind(error: &AgentLoopError) -> &'static str {
    match error {
        AgentLoopError::InvalidConfig(_) => "invalid_config",
        AgentLoopError::Cancelled => "cancelled",
        AgentLoopError::InvalidSession(_) => "invalid_session",
        AgentLoopError::Llm(_) => "llm",
        AgentLoopError::EmptyStream => "empty_stream",
        AgentLoopError::NoProgress => "no_progress",
        AgentLoopError::MaxRounds(_) => "max_rounds",
        AgentLoopError::ToolCallLimit { .. } => "tool_call_limit",
        AgentLoopError::ToolCallBudgetExhausted { .. } => "tool_call_budget",
        AgentLoopError::InvalidToolCall(_) => "invalid_tool_call",
        AgentLoopError::InvalidModelSequence(_) => "invalid_model_sequence",
        AgentLoopError::SemanticEventRejected => "semantic_event_rejected",
        AgentLoopError::Action(_) => "action",
    }
}

fn llm_error_kind(error: &LlmError) -> &'static str {
    match error.kind() {
        Some(LlmProviderErrorKind::Authentication) => "authentication",
        Some(LlmProviderErrorKind::InvalidRequest) => "invalid_request",
        Some(LlmProviderErrorKind::ModelUnavailable) => "model_unavailable",
        Some(LlmProviderErrorKind::Network) => "network",
        Some(LlmProviderErrorKind::Upstream) => "upstream",
        Some(LlmProviderErrorKind::StreamDivergence) => "stream_divergence",
        Some(LlmProviderErrorKind::Upload) => "upload",
        None => "cancelled",
    }
}

fn agent_error_response(session_id: &str, error: &AgentLoopError) -> AcpError {
    let mut data = serde_json::json!({
        "error": "agent_loop_failed",
        "kind": agent_error_kind(error),
        "message": error.to_string(),
        "session_id": session_id,
    });

    if let AgentLoopError::Llm(llm_error) = error {
        data["llm_kind"] = serde_json::Value::String(llm_error_kind(llm_error).to_owned());
    }

    AcpError::internal_error().data(data)
}

fn turn_service_error_response(
    session_id: &str,
    error: &agent_runtime::TurnServiceError,
) -> AcpError {
    match error {
        agent_runtime::TurnServiceError::Agent(agent_error) => {
            agent_error_response(session_id, agent_error)
        }
        agent_runtime::TurnServiceError::Persistence(persistence) => AcpError::internal_error()
            .data(serde_json::json!({
                "error": "turn_finalization_failed",
                "kind": "persistence",
                "message": persistence.to_string(),
                "session_id": session_id,
            })),
        agent_runtime::TurnServiceError::AgentAndPersistence { agent, persistence } => {
            let mut data = serde_json::json!({
                "error": "turn_failed_and_finalization_failed",
                "kind": "agent_and_persistence",
                "agent_error_kind": agent_error_kind(agent),
                "agent_message": agent.to_string(),
                "persistence_message": persistence.to_string(),
                "session_id": session_id,
            });
            if let AgentLoopError::Llm(llm_error) = agent {
                data["llm_kind"] = serde_json::Value::String(llm_error_kind(llm_error).to_owned());
            }
            AcpError::internal_error().data(data)
        }
    }
}

pub async fn run_turn(
    ctx: TurnContext<'_>,
    req: PromptRequest,
) -> Result<PromptResponse, AcpError> {
    let session_id = req.session_id.clone();
    let sid = &*session_id.0;
    let span = tracing::info_span!(
        "turn",
        session=%session_id,
        chars_input=tracing::field::Empty,
        chars_output=tracing::field::Empty,
        tool_rounds=tracing::field::Empty,
        outcome=tracing::field::Empty,
        agent_error_kind=tracing::field::Empty,
    );
    let _enter = span.enter();

    if let Err(error) = SessionManager::validate_id(sid) {
        fail_before_execution(ctx.semantic);
        tracing::warn!(session=%session_id, error=%error, "rejected invalid session id before persistence access");
        return Err(AcpError::invalid_params().data(serde_json::json!({
            "session_id": session_id.to_string(),
            "error": "invalid session id"
        })));
    }

    let (mut session, generation) = match ctx.store.begin_turn(sid).await {
        Ok(turn) => turn,
        Err(TurnError::NotFound(_)) => {
            fail_before_execution(ctx.semantic);
            // I-10 : payload cohérent avec les autres erreurs (champ `error`).
            return Err(AcpError::invalid_params().data(serde_json::json!({
                "session_id": session_id.to_string(),
                "error": "session not found"
            })));
        }
        Err(TurnError::AlreadyRunning) => {
            fail_before_execution(ctx.semantic);
            return Err(AcpError::invalid_params().data(serde_json::json!({"session_id": session_id.to_string(), "error": "a turn is already running; send session/cancel first"})));
        }
        Err(TurnError::BusyIo(message)) => {
            fail_before_execution(ctx.semantic);
            // Storage fault, not a concurrency signal: projected as an
            // internal error so the user never gets told to cancel a turn
            // that does not exist.
            return Err(AcpError::internal_error().data(serde_json::json!({
                "session_id": session_id.to_string(),
                "error": "session_storage_io_failure",
                "message": message,
            })));
        }
    };

    let (user_text, images) = blocks_to_parts(&req.prompt);
    span.record("chars_input", user_text.chars().count());

    if session.title.is_none() && !user_text.trim().is_empty() {
        let title = derive_title(&user_text);
        // Persistence invariant: the store's live entry must receive the title
        // here. `end_turn` merges the live entry's title into the final commit,
        // so a session that only ever notified the UI lost its title on
        // restart or fork (session/list and session/load returned None).
        let persisted = ctx
            .store
            .update_session(sid, |s| s.title = Some(title.clone()))
            .await;
        if let Err(error) = persisted {
            tracing::warn!(session=%session_id, error=%error, "failed to persist derived session title");
        }
        safe_session_update(
            &ctx.cx,
            &session_id,
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title)),
        );
    }

    let refs = match uploads::upload_images(&*ctx.llm, Some(&ctx.cx), &session_id, &images).await {
        Ok(refs) => refs,
        Err(upload_error) => {
            fail_before_execution(ctx.semantic);
            // The user message is pushed BEFORE finalizing so it survives the
            // replay: the failed turn still shows what the user asked for.
            session.messages.push((Role::User, user_text));
            if let Err(error) = ctx.store.end_turn(sid, session, generation).await {
                tracing::warn!(session=%session_id, error=%error, "turn finalization failed after image upload failure");
            }
            return Err(AcpError::internal_error().data(serde_json::json!({
                "session_id": session_id.to_string(),
                "error": "image_upload_failed",
                "message": upload_error.message,
                "image_index": upload_error.index,
                "image_total": upload_error.total,
            })));
        }
    };

    session.messages.push((Role::User, user_text));

    let action_handler = action::shared(ctx.cx.clone(), session_id.clone());
    let permission_handler = std::sync::Arc::new(AcpToolPermissionHandler::new(
        ctx.cx.clone(),
        ctx.store.clone(),
    ));

    let result = ctx
        .turn_service
        .run_started(TurnExecutionRequest {
            session_id: sid.to_string(),
            session,
            generation,
            references: refs,
            cancellation: ctx.cancellation.clone(),
            semantic: ctx.semantic,
            action_handler: Some(action_handler),
            permission_handler: Some(permission_handler),
            build_prompt: build_prompt_for_agent_loop,
        })
        .await;

    match result {
        Ok(result) => {
            span.record("tool_rounds", result.outcome.rounds);
            span.record("chars_output", result.outcome.output.chars().count());
            span.record("outcome", "success");
            let usage_prompt =
                crate::prompt::build::build_prompt(&result.session, Some(&*ctx.tools));
            if let Err(error) =
                notify_usage(&ctx.cx, &session_id, &usage_prompt, &result.outcome.output)
            {
                tracing::warn!(session=%session_id, error=%error, "notify_usage failed after successful turn");
            }
            Ok(PromptResponse::new(StopReason::EndTurn))
        }
        Err(error) => {
            let outcome = match &error {
                agent_runtime::TurnServiceError::Agent(agent_error) => {
                    agent_error_kind(agent_error)
                }
                agent_runtime::TurnServiceError::Persistence(_) => "persistence",
                agent_runtime::TurnServiceError::AgentAndPersistence { .. } => {
                    "agent_and_persistence"
                }
            };
            span.record("outcome", outcome);
            if let agent_runtime::TurnServiceError::Agent(agent_error) = &error {
                span.record("agent_error_kind", agent_error_kind(agent_error));
            }
            tracing::error!(session=%session_id, error=%error, "turn execution failed");

            if let agent_runtime::TurnServiceError::Agent(agent_error) = &error {
                if let Some(reason) = map_agent_error(agent_error) {
                    return Ok(PromptResponse::new(reason));
                }
            }
            Err(turn_service_error_response(&session_id.to_string(), &error))
        }
    }
}

fn build_prompt_for_agent_loop(
    session: &agent_runtime::state::Session,
    provider: &dyn agent_runtime::ToolProvider,
) -> String {
    crate::prompt::build::build_prompt(session, Some(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::AgentActionError;

    #[test]
    fn invalid_session_id_is_rejected_before_store_access() {
        assert!(SessionManager::validate_id("../../escape").is_err());
        assert!(SessionManager::validate_id("sess_../escape").is_err());
        assert!(SessionManager::validate_id("sess_00000000000000000000000000000000").is_ok());
    }

    #[test]
    fn only_protocol_level_terminations_map_to_stop_reasons() {
        assert_eq!(
            map_agent_error(&AgentLoopError::Cancelled),
            Some(StopReason::Cancelled)
        );
        assert_eq!(
            map_agent_error(&AgentLoopError::MaxRounds(20)),
            Some(StopReason::MaxTurnRequests)
        );
        assert_eq!(map_agent_error(&AgentLoopError::EmptyStream), None);
        assert_eq!(map_agent_error(&AgentLoopError::NoProgress), None);
        assert_eq!(
            map_agent_error(&AgentLoopError::SemanticEventRejected),
            None
        );
        assert_eq!(
            map_agent_error(&AgentLoopError::InvalidModelSequence("broken".into())),
            None
        );
        assert_eq!(
            map_agent_error(&AgentLoopError::Action(AgentActionError::Failed(
                "boom".into()
            ))),
            None
        );
    }

    #[test]
    fn error_kind_is_stable_and_machine_readable() {
        assert_eq!(
            agent_error_kind(&AgentLoopError::EmptyStream),
            "empty_stream"
        );
        assert_eq!(agent_error_kind(&AgentLoopError::NoProgress), "no_progress");
        assert_eq!(
            agent_error_kind(&AgentLoopError::SemanticEventRejected),
            "semantic_event_rejected"
        );
        assert_eq!(
            agent_error_kind(&AgentLoopError::MaxRounds(3)),
            "max_rounds"
        );
    }

    #[test]
    fn llm_error_kind_is_stable_and_machine_readable() {
        assert_eq!(
            llm_error_kind(&LlmError::Authentication("expired".into())),
            "authentication"
        );
        assert_eq!(
            llm_error_kind(&LlmError::Unavailable("gemini-3".into())),
            "model_unavailable"
        );
        assert_eq!(
            llm_error_kind(&LlmError::Network("timeout".into())),
            "network"
        );
        assert_eq!(
            llm_error_kind(&LlmError::StreamDivergence),
            "stream_divergence"
        );
    }
}
