mod context;
mod guard;
mod permission;

use super::action;
use super::content::blocks_to_parts;
use super::notify::notify_usage;
use super::title::derive_title;
use permission::AcpToolPermissionHandler;
use agent_client_protocol::schema::v1::{PromptRequest, PromptResponse, SessionInfoUpdate, SessionUpdate, StopReason};
use agent_client_protocol::Responder;
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

fn map_agent_error(error: &agent_runtime::AgentLoopError) -> StopReason {
    match error {
        agent_runtime::AgentLoopError::Cancelled => StopReason::Cancelled,
        agent_runtime::AgentLoopError::MaxRounds(_) => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    }
}

pub async fn run_turn(
    ctx: TurnContext<'_>,
    req: PromptRequest,
    responder: Responder<PromptResponse>,
) -> Result<(), AcpError> {
    let session_id = req.session_id.clone();
    let sid = &*session_id.0;
    let span = tracing::info_span!("turn", session=%session_id, chars_input=tracing::field::Empty, chars_output=tracing::field::Empty, tool_rounds=tracing::field::Empty, outcome=tracing::field::Empty);
    let _enter = span.enter();
    let (session, _store_cancel, generation) = match ctx.store.begin_turn(sid).await {
        Ok(turn) => turn,
        Err(TurnError::NotFound(_)) => {
            fail_before_execution(ctx.semantic);
            return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id": session_id.to_string()})));
        }
        Err(TurnError::AlreadyRunning) => {
            fail_before_execution(ctx.semantic);
            return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id": session_id.to_string(), "error": "a turn is already running; send session/cancel first"})));
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
            return responder.respond(PromptResponse::new(StopReason::Refusal));
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
            let usage_prompt = {
                let session = guard.session_mut();
                crate::prompt::build::build_prompt(session, Some(&*ctx.tools))
            };
            if let Err(error) = notify_usage(&ctx.cx, &session_id, &usage_prompt, &outcome.output) {
                tracing::warn!(session=%session_id, "notify_usage a échoué: {error}");
            }
            guard.finish().await;
            responder.respond(PromptResponse::new(StopReason::EndTurn))
        }
        Err(error) => {
            let reason = map_agent_error(&error);
            if !ctx.semantic.is_terminal() {
                if matches!(error, agent_runtime::AgentLoopError::Cancelled) { ctx.semantic.turn_cancelled(); }
                else { ctx.semantic.turn_failed(); }
            }
            guard.finish().await;
            responder.respond(PromptResponse::new(reason))
        }
    }
}
