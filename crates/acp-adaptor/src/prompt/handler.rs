use crate::prompt;
use agent_client_protocol::schema::v1::{MessageId, PromptRequest, PromptResponse};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};
use agent_runtime::events::TurnEventEmitter;
use agent_runtime::{AppState, RuntimeError};
use tools_provider::tools::interactive;

/// Owns ACP-specific prompt orchestration: turn ownership, semantic projection,
/// interactive scope and response delivery. Runtime execution itself remains in
/// `agent_runtime::TurnService`.
pub async fn handle_prompt(
    req: PromptRequest,
    responder: Responder<PromptResponse>,
    cx: ConnectionTo<Client>,
    state: AppState,
) -> Result<(), RuntimeError> {
    let session_id = req.session_id.clone();
    let sid = session_id.0.to_string();
    let turn_service = state.turn_service.clone();
    let turn_manager = state.turns.clone();
    let events = state.events.clone();

    turn_manager
        .start(sid.clone(), move |cancellation| async move {
            let turn_id = format!("turn_{}", uuid::Uuid::new_v4().simple());
            let projection_rx = events.subscribe_turn(&turn_id);
            let projection_cx = cx.clone();
            let projection_session_id = session_id.clone();
            let projection_message_id = MessageId::from(format!("msg_{turn_id}"));
            let projection_cancellation = cancellation.clone();
            let projection_turn_id = turn_id.clone();
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
                cx: cx.clone(),
                session_id: session_id.clone(),
            };

            interactive::scope(interactive_context, async move {
                let mut semantic = TurnEventEmitter::new_with_required_transport(
                    events.clone(),
                    sid.clone(),
                    turn_id.clone(),
                );
                let turn_context = prompt::TurnContext {
                    store: state.store.clone(),
                    tools: state.tools.clone(),
                    llm: state.llm.clone(),
                    turn_service,
                    cx: cx.clone(),
                    semantic: &mut semantic,
                    cancellation,
                };
                let turn_result = prompt::run_turn(turn_context, req).await;

                if turn_result.is_err() && !semantic.is_terminal() {
                    let _ = semantic.turn_failed();
                }

                let projection_result = projection.await;
                events.close_turn(&turn_id);

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
