mod context;
mod guard;
mod permission;

use super::action;
use super::content::blocks_to_parts;
use super::notify::notify_usage;
use super::title::derive_title;
use permission::AcpToolPermissionHandler;
use agent_client_protocol::schema::v1::{PromptRequest, PromptResponse, SessionInfoUpdate, SessionUpdate, StopReason};
use agent_client_protocol::Error as AcpError;
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::state::{Role, TurnError};
use agent_runtime::{AgentLoop, AgentLoopConfig};
use tools_provider::tools::executor::safe_session_update;
use super::turn_context::TurnContext;

fn fail_before_execution(semantic: &mut TurnEventEmitter) {
    if semantic.is_terminal() { return; }
    let _ = semantic.turn_started();
    let _ = semantic.turn_failed();
}

fn map_agent_error(error: &agent_runtime::AgentLoopError) -> Option<StopReason> {
    match error {
        agent_runtime::AgentLoopError::Cancelled => Some(StopReason::Cancelled),
        agent_runtime::AgentLoopError::MaxRounds(_) => Some(StopReason::MaxTokens),
        _ => None,
    }
}

fn agent_error_kind(error: &agent_runtime::AgentLoopError) -> &'static str {
    match error {
        agent_runtime::AgentLoopError::InvalidConfig(_) => "invalid_config",
        agent_runtime::AgentLoopError::Cancelled => "cancelled",
        agent_runtime::AgentLoopError::InvalidSession(_) => "invalid_session",
        agent_runtime::AgentLoopError::Llm(_) => "llm",
        agent_runtime::AgentLoopError::EmptyStream => "empty_stream",
        agent_runtime::AgentLoopError::NoProgress => "no_progress",
        agent_runtime::AgentLoopError::MaxRounds(_) => "max_rounds",
        agent_runtime::AgentLoopError::ToolCallLimit { .. } => "tool_call_limit",
        agent_runtime::AgentLoopError::InvalidToolCall(_) => "invalid_tool_call",
        agent_runtime::AgentLoopError::InvalidModelSequence(_) => "invalid_model_sequence",
        agent_runtime::AgentLoopError::SemanticEventRejected => "semantic_event_rejected",
        agent_runtime::AgentLoopError::Action(_) => "action",
    }
}

fn agent_error_response(session_id: &str, error: &agent_runtime::AgentLoopError) -> AcpError {
    AcpError::internal_error().data(serde_json::json!({
        "error": "agent_loop_failed",
        "kind": agent_error_kind(error),
        "message": error.to_string(),
        "session_id": session_id,
    }))
}

pub async fn run_turn(ctx: TurnContext<'_>, req: PromptRequest) -> Result<PromptResponse, AcpError> {
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
    let (session, generation) = match ctx.store.begin_turn(sid).await {
        Ok(turn) => turn,
        Err(TurnError::NotFound(_)) => {
            fail_before_execution(ctx.semantic);
            return Err(AcpError::invalid_params().data(serde_json::json!({"session_id": session_id.to_string()})));
        }
        Err(TurnError::AlreadyRunning) => {
            fail_before_execution(ctx.semantic);
            return Err(AcpError::invalid_params().data(serde_json::json!({"session_id": session_id.to_string(), "error": "a turn is already running; send session/cancel first"})));
        }
    };

    let mut guard = guard::TurnGuard::new(ctx.store.clone(), sid.to_string(), session, generation);
    let (user_text, images) = blocks_to_parts(&req.prompt);
    span.record("chars_input", user_text.chars().count());

    {
        let session = guard.session_mut();
        if session.title.is_none() && !user_text.trim().is_empty() {
            let title = derive_title(&user_text);
            session.title = Some(title.clone());
            safe_session_update(&ctx.cx, &session_id, SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title)));
        }
    }

    let refs = match context::upload_images(&*ctx.llm, &ctx.cx, &session_id, &images).await {
        Ok(refs) => refs,
        Err(()) => {
            fail_before_execution(ctx.semantic);
            guard.finish().await;
            return Ok(PromptResponse::new(StopReason::Refusal));
        }
    };

    {
        let session = guard.session_mut();
        session.messages.push((Role::User, user_text));
    }

    let action_handler = action::shared(ctx.cx.clone(), session_id.clone());
    let permission_handler = std::sync::Arc::new(AcpToolPermissionHandler::new(ctx.cx.clone(), ctx.tools.clone()));
    let agent_loop = match AgentLoop::new(ctx.llm.clone(), ctx.tools.clone(), AgentLoopConfig::default()) {
        Ok(loop_) => loop_.with_action_handler(action_handler).with_permission_handler(permission_handler),
        Err(error) => {
            fail_before_execution(ctx.semantic);
            guard.finish().await;
            return Err(AcpError::internal_error().data(serde_json::json!({"error": error.to_string()})));
        }
    };

    let result = {
        let session = guard.session_mut();
        agent_loop.run(
            session,
            &refs,
            ctx.cancellation.clone(),
            ctx.semantic,
            |session, provider| crate::prompt::build::build_prompt(session, Some(provider)),
        ).await
    };

    match result {
        Ok(outcome) => {
            span.record("tool_rounds", outcome.rounds);
            span.record("chars_output", outcome.output.chars().count());
            span.record("outcome", "success");
            let usage_prompt = {
                let session = guard.session_mut();
                crate::prompt::build::build_prompt(session, Some(&*ctx.tools))
            };
            if let Err(error) = notify_usage(&ctx.cx, &session_id, &usage_prompt, &outcome.output) {
                tracing::warn!(session=%session_id, error=%error, "notify_usage failed after successful turn");
            }
            guard.finish().await;
            Ok(PromptResponse::new(StopReason::EndTurn))
        }
        Err(error) => {
            let kind = agent_error_kind(&error);
            span.record("agent_error_kind", kind);
            span.record("outcome", kind);
            if let agent_runtime::AgentLoopError::MaxRounds(limit) = &error {
                tracing::error!(session=%session_id, error_kind=%kind, max_rounds=*limit, error=%error, "agent loop exhausted its round limit");
            } else {
                tracing::error!(session=%session_id, error_kind=%kind, error=%error, "agent loop failed");
            }

            if !ctx.semantic.is_terminal() {
                if matches!(error, agent_runtime::AgentLoopError::Cancelled) {
                    ctx.semantic.turn_cancelled();
                } else {
                    ctx.semantic.turn_failed();
                }
            }
            guard.finish().await;

            match map_agent_error(&error) {
                Some(reason) => Ok(PromptResponse::new(reason)),
                None => Err(agent_error_response(&session_id.to_string(), &error)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::AgentLoopError;

    #[test]
    fn only_protocol_level_terminations_map_to_stop_reasons() {
        assert_eq!(map_agent_error(&AgentLoopError::Cancelled), Some(StopReason::Cancelled));
        assert_eq!(map_agent_error(&AgentLoopError::MaxRounds(20)), Some(StopReason::MaxTokens));
        assert_eq!(map_agent_error(&AgentLoopError::EmptyStream), None);
        assert_eq!(map_agent_error(&AgentLoopError::NoProgress), None);
        assert_eq!(map_agent_error(&AgentLoopError::SemanticEventRejected), None);
        assert_eq!(map_agent_error(&AgentLoopError::InvalidModelSequence("broken".into())), None);
        assert_eq!(map_agent_error(&AgentLoopError::Action("boom".into())), None);
    }

    #[test]
    fn error_kind_is_stable_and_machine_readable() {
        assert_eq!(agent_error_kind(&AgentLoopError::EmptyStream), "empty_stream");
        assert_eq!(agent_error_kind(&AgentLoopError::NoProgress), "no_progress");
        assert_eq!(agent_error_kind(&AgentLoopError::SemanticEventRejected), "semantic_event_rejected");
        assert_eq!(agent_error_kind(&AgentLoopError::MaxRounds(3)), "max_rounds");
    }
}
