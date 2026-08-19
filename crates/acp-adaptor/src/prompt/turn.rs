mod context;
mod guard;
mod rounds;

use super::content::blocks_to_parts;
use super::notify::notify_usage;
use super::title::derive_title;
use agent_client_protocol::schema::v1::{
    MessageId, PromptRequest, PromptResponse, SessionInfoUpdate, SessionUpdate, StopReason,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::state::{Role, Store, TurnError};
use agent_runtime::{AgentLoopConfig, LlmProvider, ToolProvider};
use tools_provider::tools::executor::safe_session_update;
use rounds::{RoundContext, RoundOutcome};
use std::sync::Arc;

pub async fn run_turn(
    store: Arc<Store>,
    tools: Arc<dyn ToolProvider>,
    llm: Arc<dyn LlmProvider>,
    req: PromptRequest,
    responder: Responder<PromptResponse>,
    cx: ConnectionTo<Client>,
    semantic: &mut TurnEventEmitter,
) -> Result<(), AcpError> {
    if !semantic.turn_started() {
        return Err(AcpError::internal_error().data(serde_json::json!({
            "error": "semantic turn lifecycle could not be started"
        })));
    }

    let session_id = req.session_id.clone();
    let sid = &*session_id.0;
    let span = tracing::info_span!(
        "turn",
        session=%session_id,
        chars_input=tracing::field::Empty,
        chars_output=tracing::field::Empty,
        tool_rounds=tracing::field::Empty,
        outcome=tracing::field::Empty,
    );
    let _enter = span.enter();

    let (session, mut cancel, generation) = match store.begin_turn(sid).await {
        Ok(turn) => turn,
        Err(TurnError::NotFound(_)) => {
            semantic.turn_failed();
            return responder.respond_with_error(
                AcpError::invalid_params()
                    .data(serde_json::json!({"session_id": session_id.to_string()})),
            );
        }
        Err(TurnError::AlreadyRunning) => {
            semantic.turn_failed();
            return responder.respond_with_error(
                AcpError::invalid_params().data(serde_json::json!({
                    "session_id": session_id.to_string(),
                    "error": "a turn is already running; send session/cancel first"
                })),
            );
        }
    };

    let mut guard = guard::TurnGuard::new(store.clone(), sid.to_string(), session, generation);
    let session = guard.session_mut();
    let (user_text, images) = blocks_to_parts(&req.prompt);
    span.record("chars_input", user_text.chars().count());
    let message_id = MessageId::from(format!("msg_{}", uuid::Uuid::new_v4().simple()));

    if session.title.is_none() && !user_text.trim().is_empty() {
        let title = derive_title(&user_text);
        session.title = Some(title.clone());
        safe_session_update(
            &cx,
            &session_id,
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title)),
        );
    }

    let refs = match context::upload_images(&*llm, &cx, &session_id, &images).await {
        Ok(refs) => refs,
        Err(()) => {
            semantic.turn_failed();
            return responder.respond(PromptResponse::new(StopReason::Refusal));
        }
    };

    session.messages.push((Role::User, user_text));

    let (output, tool_round, assistant_already_persisted) = {
        let max_rounds = AgentLoopConfig::default().max_rounds;
        let mut round_context = RoundContext {
            llm: &*llm,
            cx: &cx,
            session_id: &session_id,
            sid,
            message_id: &message_id,
            cancel: &mut cancel,
            session,
            provider: &*tools,
            semantic,
            refs: &refs,
            span: &span,
        };

        let outcome = match rounds::run(&mut round_context, max_rounds).await {
            Ok(outcome) => outcome,
            Err(rounds::RoundError::Stop(reason)) => {
                match reason {
                    StopReason::Cancelled => round_context.semantic.turn_cancelled(),
                    _ => round_context.semantic.turn_failed(),
                };
                return responder.respond(PromptResponse::new(reason));
            }
            Err(rounds::RoundError::Acp(error)) => {
                round_context.semantic.turn_failed();
                return Err(error);
            }
        };

        let RoundOutcome {
            output,
            tool_round,
            assistant_already_persisted,
        } = outcome;
        span.record("tool_rounds", tool_round);
        span.record("chars_output", output.chars().count());

        if !assistant_already_persisted && !output.trim().is_empty() {
            round_context
                .session
                .messages
                .push((Role::Assistant, output.clone()));
        }

        if let Err(error) = notify_usage(
            &cx,
            &session_id,
            &crate::prompt::build::build_prompt(round_context.session, Some(round_context.provider)),
            &output,
        ) {
            tracing::warn!(session=%session_id,"notify_usage a échoué: {error}");
        }

        (output, tool_round, assistant_already_persisted)
    };

    let _ = (tool_round, assistant_already_persisted);
    guard.finish().await;
    semantic.turn_completed();
    responder.respond(PromptResponse::new(StopReason::EndTurn))
}
