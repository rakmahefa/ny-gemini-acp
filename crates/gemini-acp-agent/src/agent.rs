//! Construction de l'agent ACP et câblage du transport stdio.
//!
//! Refactor R1 — inspiré de `glm-acp-agent/src/protocol/agent.ts` :
//!
//! - **Fork session** : ajout du handler `session/fork`.
//! - **Set session mode** : ajout du handler `session/set_mode`.
//! - **Capabilities enrichies** : annonce `fork` et `mcpCapabilities`.
//! - **Prompt serialization** : les tours sont maintenant sérialisés par
//!   `gemini-acp-encaps::TurnManager`, un verrou logique par session.
//! - **Interactive tool context** : chaque tour reçoit un contexte ACP task-local.
//! - **Refactor 3-crates** : l'agent réutilise directement `AppState` du runtime.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Agent, Error as AcpError, Stdio};
use gemini_acp_encaps::TurnManager;
use gemini_acp_runtime::AppState;

use crate::handlers;
use crate::prompt;

/// Construit l'agent ACP et le lance sur le transport stdio.
pub async fn run_agent(state: AppState) -> Result<(), AcpError> {
    let h_store = state.store.clone();
    let h_client = state.client.clone();
    let h_tools = state.tools.clone();
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
        // session/prompt : la sérialisation est désormais portée par encaps.
        .on_receive_request(
            {
                let store = h_store.clone();
                let client = h_client.clone();
                let tools = h_tools.clone();
                let turn_manager = turn_manager.clone();
                async move |req: PromptRequest, responder, cx| {
                    let store = store.clone();
                    let client = client.clone();
                    let tools = tools.clone();
                    let turn_manager = turn_manager.clone();
                    let turn_cx = cx.clone();
                    let sid = req.session_id.0.clone();
                    let session_id = req.session_id.clone();

                    let _ = turn_manager
                        .start(sid, move |cancellation| async move {
                            let mut cancel_rx = cancellation.subscribe();
                            let interactive = gemini_acp_runtime::tools::interactive::InteractiveContext {
                                cx: turn_cx.clone(),
                                session_id,
                            };
                            let future = gemini_acp_runtime::tools::interactive::scope(
                                interactive,
                                async move {
                                    prompt::run_turn(store, tools, client, req, responder, turn_cx).await
                                },
                            );

                            tokio::select! {
                                result = future => result.map_err(|e| gemini_acp_encaps::EncapsError::Task(e.to_string())),
                                _ = cancel_rx.changed() => Ok(()),
                            }
                        })
                        .await
                        .map_err(|error| tracing::error!(%error, "failed to enqueue ACP turn"));
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
