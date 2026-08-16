//! Construction de l'agent ACP et câblage du transport stdio.
//!
//! Refactor R1 — inspiré de `glm-acp-agent/src/protocol/agent.ts` :
//! - Fork session et set mode.
//! - Prompt serialization via `gemini-acp-encaps::TurnManager`.
//! - Interactive tool context isolé par tour.
//! - AppState fourni par `gemini-acp-runtime`.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Agent, Error as AcpError, Stdio};
use gemini_acp_encaps::TurnManager;
use gemini_acp_runtime::{events::TurnEventEmitter, AppState};

use crate::handlers;
use crate::prompt;

pub async fn run_agent(state: AppState) -> Result<(), AcpError> {
    let h_store = state.store.clone();
    let h_client = state.client.clone();
    let h_tools = state.tools.clone();
    let h_events = state.events.clone();
    let turn_manager = TurnManager::new();

    Agent
        .builder()
        .name("gemini-acp")
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: InitializeRequest, responder, _cx| {
                    handlers::init::handle(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: NewSessionRequest, responder, _cx| {
                    handlers::session::handle_new(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // session/prompt : TurnManager remplace wait_prompt_done + prompt_handle.
        .on_receive_request(
            {
                let store = h_store.clone();
                let client = h_client.clone();
                let tools = h_tools.clone();
                let events = h_events.clone();
                let turn_manager = turn_manager.clone();
                async move |req: PromptRequest, responder, cx| {
                    let store = store.clone();
                    let client = client.clone();
                    let tools = tools.clone();
                    let events = events.clone();
                    let turn_manager = turn_manager.clone();
                    let turn_cx = cx.clone();
                    let sid = req.session_id.0.to_string();
                    let session_id = req.session_id.clone();

                    turn_manager
                        .start(sid.clone(), move |_cancellation| async move {
                            let turn_id = format!("turn_{}", uuid::Uuid::new_v4().simple());
                            let interactive =
                                gemini_acp_runtime::tools::interactive::InteractiveContext {
                                    cx: turn_cx.clone(),
                                    session_id,
                                };

                            gemini_acp_runtime::tools::interactive::scope(
                                interactive,
                                async move {
                                    let mut semantic =
                                        TurnEventEmitter::new(events, sid.clone(), turn_id);
                                    semantic.turn_started();

                                    let result = prompt::run_turn(
                                        store,
                                        tools,
                                        client,
                                        req,
                                        responder,
                                        turn_cx,
                                        &mut semantic,
                                    )
                                    .await
                                    .map_err(|e| {
                                        gemini_acp_encaps::EncapsError::Task(e.to_string())
                                    });

                                    // `run_turn` owns all expected terminal outcomes. Keep a
                                    // defensive failure transition for an unexpected orchestration
                                    // error so an active semantic turn can never leak terminality.
                                    if result.is_err() && matches!(
                                        semantic.phase(),
                                        gemini_acp_runtime::events::integrity::TurnPhase::Active
                                    ) {
                                        semantic.turn_failed();
                                    }

                                    result
                                },
                            )
                            .await
                        })
                        .await
                        .map_err(|error| anyhow::anyhow!("failed to enqueue ACP turn: {error}"))?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ListSessionsRequest, responder, _cx| {
                    handlers::session::handle_list(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: LoadSessionRequest, responder, cx| {
                    handlers::session::handle_load(req, responder, &state, &cx).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ResumeSessionRequest, responder, cx| {
                    handlers::session::handle_resume(req, responder, &state, &cx).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: DeleteSessionRequest, responder, _cx| {
                    handlers::session::handle_delete(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: CloseSessionRequest, responder, _cx| {
                    handlers::session::handle_close(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: SetSessionConfigOptionRequest, responder, cx| {
                    handlers::config::handle(req, responder, &state, &cx).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: SetSessionModeRequest, responder, cx| {
                    handlers::session::handle_set_mode(req, responder, &state, &cx).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ForkSessionRequest, responder, _cx| {
                    handlers::session::handle_fork(req, responder, &state).await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                let turn_manager = turn_manager.clone();
                async move |notif: CancelNotification, _cx| {
                    handlers::cancel::handle(notif, &state, &turn_manager).await
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}
