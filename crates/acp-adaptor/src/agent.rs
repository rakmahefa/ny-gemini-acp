use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Agent, Error as AcpError, Stdio};
use agent_runtime::{AppState, EncapsError, ToolProvider, TurnManager};
use agent_runtime::events::TurnEventEmitter;
use crate::{handlers, prompt};

pub async fn run_agent(state: AppState) -> Result<(), AcpError> {
    let h_store = state.store.clone();
    let h_tools = state.tools.clone();
    let h_sessions = state.sessions.clone();
    let h_llm = state.llm.clone();
    let h_events = state.events.clone();
    let turn_manager = TurnManager::new();

    Agent::builder(Agent::default()).name("gemini-acp")
        .on_receive_request({
            let state = state.clone();
            async move |req: InitializeRequest, responder, _cx| {
                handlers::init::handle(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: NewSessionRequest, responder, _cx| {
                handlers::session::handle_new(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let store = h_store.clone();
            let tools = h_tools.clone();
            let sessions = h_sessions.clone();
            let llm = h_llm.clone();
            let events = h_events.clone();
            let turn_manager = turn_manager.clone();
            async move |req: PromptRequest, responder, cx| {
                let store = store.clone();
                let fallback_tools = tools.clone();
                let sessions = sessions.clone();
                let llm = llm.clone();
                let events = events.clone();
                let turn_manager = turn_manager.clone();
                let turn_cx = cx.clone();
                let sid = req.session_id.0.to_string();
                let session_id = req.session_id.clone();

                turn_manager
                    .start(sid.clone(), move |cancellation| async move {
                        let tools_for_session = sessions.tools_for(&sid).await;
                        let tools: std::sync::Arc<dyn ToolProvider> =
                            if tools_for_session.has_tools() {
                                tools_for_session
                            } else {
                                fallback_tools
                            };
                        let turn_id = format!("turn_{}", uuid::Uuid::new_v4().simple());
                        let interactive = gemini_acp_tools::tools::interactive::InteractiveContext {
                            cx: turn_cx.clone(),
                            session_id,
                        };

                        gemini_acp_tools::tools::interactive::scope(
                            interactive,
                            async move {
                                let mut semantic = TurnEventEmitter::new(events, sid.clone(), turn_id);
                                semantic.turn_started();

                                let result = prompt::run_turn(
                                    store,
                                    tools,
                                    llm,
                                    req,
                                    responder,
                                    turn_cx,
                                    cancellation,
                                    &mut semantic,
                                )
                                .await
                                .map_err(|e| EncapsError::Task(e.to_string()));

                                if result.is_err() && !semantic.is_terminal() {
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
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: ListSessionsRequest, responder, _cx| {
                handlers::session::handle_list(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: LoadSessionRequest, responder, cx| {
                handlers::session::handle_load(req, responder, &state, &cx).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: ResumeSessionRequest, responder, cx| {
                handlers::session::handle_resume(req, responder, &state, &cx).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: DeleteSessionRequest, responder, _cx| {
                handlers::session::handle_delete(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: CloseSessionRequest, responder, _cx| {
                handlers::session::handle_close(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: SetSessionConfigOptionRequest, responder, cx| {
                handlers::config::handle(req, responder, &state, &cx).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: SetSessionModeRequest, responder, cx| {
                handlers::session::handle_set_mode(req, responder, &state, &cx).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let state = state.clone();
            async move |req: ForkSessionRequest, responder, _cx| {
                handlers::session::handle_fork(req, responder, &state).await
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_notification({
            let state = state.clone();
            let turn_manager = turn_manager.clone();
            async move |notif: CancelNotification, _cx| {
                handlers::cancel::handle(notif, &state, &turn_manager).await
            }
        }, agent_client_protocol::on_receive_notification!())
        .connect_to(Stdio::new())
        .await
}
