//! Orchestration d'un tour de conversation.
//!
//! `turn.rs` coordonne uniquement le cycle de haut niveau. Les responsabilités
//! internes sont isolées dans des sous-modules : garde de session, préparation
//! des entrées et exécution des rounds.

mod context;
mod guard;
mod rounds;

use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    MessageId, PromptRequest, PromptResponse, SessionInfoUpdate, SessionUpdate, StopReason,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};
use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::state::{SessionMode, Store, TurnError};
use gemini_acp_runtime::tools::ToolRegistry;

use super::build::build_prompt;
use super::content::blocks_to_parts;
use super::notify::notify_usage;
use super::title::derive_title;
use gemini_acp_runtime::tools::executor::safe_session_update;
use rounds::RoundOutcome;

const MAX_TURNS: usize = 20;

pub async fn run_turn(
    store: Arc<Store>,
    tools: Arc<ToolRegistry>,
    client: gemini_acp_config::client::Client,
    req: PromptRequest,
    responder: Responder<PromptResponse>,
    cx: ConnectionTo<Client>,
    semantic: &mut TurnEventEmitter,
) -> Result<(), AcpError> {
    let session_id = req.session_id.clone();
    let sid = &*session_id.0;
    let span = tracing::info_span!(
        "turn",
        session = %session_id,
        chars_input = tracing::field::Empty,
        chars_output = tracing::field::Empty,
        tool_rounds = tracing::field::Empty,
        outcome = tracing::field::Empty,
    );
    let _enter = span.enter();

    let (session, mut cancel, generation) = match store.begin_turn(sid).await {
        Ok(triple) => triple,
        Err(TurnError::NotFound(_)) => {
            semantic.turn_failed();
            return responder.respond_with_error(
                AcpError::invalid_params()
                    .data(serde_json::json!({ "session_id": session_id.to_string() })),
            );
        }
        Err(TurnError::AlreadyRunning) => {
            semantic.turn_failed();
            return responder.respond_with_error(AcpError::invalid_params().data(
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "error": "a turn is already running; send session/cancel first",
                }),
            ));
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

    let refs = match context::upload_images(&client, &cx, &session_id, &images).await {
        Ok(refs) => refs,
        Err(()) => {
            semantic.turn_failed();
            span.record("outcome", "refusal_upload");
            return responder.respond(PromptResponse::new(StopReason::Refusal));
        }
    };

    session
        .messages
        .push((gemini_acp_runtime::state::Role::User, user_text));

    let cwd = session.cwd.clone();
    let additional_dirs = session.additional_directories.clone();
    let registry = &*tools;
    let session_mode = session.mode;
    let mode_getter: Box<dyn Fn() -> SessionMode + Send + Sync> =
        Box::new(move || session_mode);

    let outcome = match rounds::run(
        &client,
        &cx,
        &session_id,
        sid,
        &message_id,
        &mut cancel,
        session,
        registry,
        semantic,
        &refs,
        &cwd,
        &additional_dirs,
        &*mode_getter,
        MAX_TURNS,
        &span,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(rounds::RoundError::Stop(reason)) => {
            return responder.respond(PromptResponse::new(reason));
        }
        Err(rounds::RoundError::Acp(error)) => return Err(error),
    };

    let RoundOutcome {
        output,
        tool_round,
        assistant_already_persisted,
    } = outcome;

    span.record("tool_rounds", tool_round);
    span.record("chars_output", output.chars().count());

    if !assistant_already_persisted && !output.trim().is_empty() {
        session
            .messages
            .push((gemini_acp_runtime::state::Role::Assistant, output.clone()));
    }

    if let Err(error) = notify_usage(
        &cx,
        &session_id,
        &build_prompt(session, Some(registry)),
        &output,
    ) {
        tracing::warn!(session = %session_id, "notify_usage a échoué: {error}");
    }

    guard.finish().await;
    span.record("outcome", "end_turn");
    semantic.turn_completed();
    responder.respond(PromptResponse::new(StopReason::EndTurn))
}
