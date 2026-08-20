use crate::{handlers, prompt};
use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Agent, Error as AcpError, Stdio};
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::{AppState, RuntimeError, TurnManager};
use tools_provider::tools::interactive;

pub async fn run_agent(state: AppState) -> Result<(), AcpError> {
    let h_store = state.store.clone();
    let h_tools = state.tools.clone();
    let h_llm = state.llm.clone();
    let h_events = state.events.clone();
    let turn_manager = TurnManager::new();

    Agent::builder(Agent)
        .name("gemini-acp")
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: InitializeRequest, responder, _cx| handlers::init::handle(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: NewSessionRequest, responder, _cx| handlers::session::handle_new(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let store = h_store.clone();
                let tools = h_tools.clone();
                let llm = h_llm.clone();
                let events = h_events.clone();
                let turn_manager = turn_manager.clone();
                async move |req: PromptRequest, responder, cx| {
                    let store = store.clone();
                    let tools = tools.clone();
                    let llm = llm.clone();
                    let events = events.clone();
                    let turn_manager = turn_manager.clone();
                    let turn_cx = cx.clone();
                    let sid = req.session_id.0.to_string();
                    let session_id = req.session_id.clone();

                    turn_manager
                        .start(sid.clone(), move |cancellation| async move {
                            let turn_id = format!("turn_{}", uuid::Uuid::new_v4().simple());
                            let projection_rx = events.subscribe();
                            let projection_cx = turn_cx.clone();
                            let projection_session_id = session_id.clone();
                            let projection_turn_id = turn_id.clone();
                            let projection_message_id = MessageId::from(format!("msg_{}", turn_id));
                            let projection_cancellation = cancellation.clone();
                            let projection = tokio::spawn(async move {
                                prompt::stream::project(
                                    projection_rx,
                                    &projection_cx,
                                    &projection_session_id,
                                    &projection_message_id,
                                    &projection_turn_id,
                                    projection_cancellation,
                                )
                                .await
                            });

                            let interactive_context = interactive::InteractiveContext {
                                cx: turn_cx.clone(),
                                session_id,
                            };

                            interactive::scope(interactive_context, async move {
                                let mut semantic = TurnEventEmitter::new(events, sid.clone(), turn_id);
                                let turn_context = prompt::TurnContext {
                                    store,
                                    tools,
                                    llm,
                                    cx: turn_cx,
                                    semantic: &mut semantic,
                                    cancellation,
                                };
                                let turn_result = prompt::run_turn(turn_context, req)
                                    .await;

                                if turn_result.is_err() && !semantic.is_terminal() {
                                    let _ = semantic.turn_failed();
                                }

                                let projection_result = projection.await;
                                let result = match projection_result {
                                    Ok(Ok(())) => turn_result,
                                    Ok(Err(error)) => {
                                        tracing::error!(error = %error, "semantic event to ACP transport failed");
                                        Err(AcpError::internal_error().data(serde_json::json!({
                                            "error": "semantic event transport failed",
                                            "details": error.to_string(),
                                        })))
                                    }
                                    Err(error) => {
                                        tracing::error!(error = %error, "semantic event projection task failed");
                                        Err(AcpError::internal_error().data(serde_json::json!({
                                            "error": "semantic event projection task failed",
                                            "details": error.to_string(),
                                        })))
                                    }
                                };

                                match result {
                                    Ok(response) => responder
                                        .respond(response)
                                        .map_err(|error| RuntimeError::Task(error.to_string())),
                                    Err(error) => responder
                                        .respond_with_error(error)
                                        .map_err(|error| RuntimeError::Task(error.to_string())),
                                }
                            })
                            .await
                        })
                        .await
                        .map_err(|error| anyhow::anyhow!("failed to enqueue agent turn: {error}"))?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ListSessionsRequest, responder, _cx| handlers::session::handle_list(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: LoadSessionRequest, responder, cx| handlers::session::handle_load(req, responder, &state, &cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ResumeSessionRequest, responder, cx| handlers::session::handle_resume(req, responder, &state, &cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: DeleteSessionRequest, responder, _cx| handlers::session::handle_delete(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: CloseSessionRequest, responder, _cx| handlers::session::handle_close(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: SetSessionConfigOptionRequest, responder, cx| handlers::config::handle(req, responder, &state, &cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: SetSessionModeRequest, responder, cx| handlers::session::handle_set_mode(req, responder, &state, &cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |req: ForkSessionRequest, responder, _cx| handlers::session::handle_fork(req, responder, &state).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                let turn_manager = turn_manager.clone();
                async move |notif: CancelNotification, _cx| handlers::cancel::handle(notif, &state, &turn_manager).await
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}
