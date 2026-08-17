use agent_client_protocol::schema::v1::{MessageId, SessionId, StopReason};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::state::{Role, Session};
use gemini_acp_runtime::tools::executor::{emit_error_chunk, ToolExecutor};
use gemini_acp_runtime::tools::parse::parse_tool_calls;
use gemini_acp_runtime::tools::ToolRegistry;
use tokio::sync::watch;

use super::context::{compact_messages, COMPACTION_THRESHOLD_CHARS, EMERGENCY_COMPACTION_CHARS};
use crate::prompt::error::actionable_error_message;
use crate::prompt::follow_up::{replace_components, request_action};
use crate::prompt::stream;
use gemini_acp_runtime::tools::lifecycle::clear_partial_output;

pub(crate) enum RoundError {
    Stop(StopReason),
    Acp(agent_client_protocol::Error),
}

pub(crate) struct RoundOutcome {
    pub(crate) output: String,
    pub(crate) tool_round: usize,
    pub(crate) assistant_already_persisted: bool,
}

pub(crate) struct RoundContext<'a> {
    pub(crate) client: &'a gemini_acp_config::client::Client,
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) sid: &'a str,
    pub(crate) message_id: &'a MessageId,
    pub(crate) cancel: &'a mut watch::Receiver<bool>,
    pub(crate) session: &'a mut Session,
    pub(crate) registry: &'a ToolRegistry,
    pub(crate) semantic: &'a mut TurnEventEmitter,
    pub(crate) refs: &'a [String],
    pub(crate) span: &'a tracing::Span,
}

fn map_stop_reason_from_error(error: &str) -> StopReason {
    let lower = error.to_lowercase();
    if lower.contains("safety") || lower.contains("block") {
        StopReason::Refusal
    } else {
        StopReason::EndTurn
    }
}

pub(crate) async fn run(
    ctx: &mut RoundContext<'_>,
    max_turns: usize,
) -> Result<RoundOutcome, RoundError> {
    let mut total_output = String::new();
    let mut tool_round = 0usize;
    let mut assistant_already_persisted = false;
    let mut overflow_retry_count = 0usize;

    for round in 0..max_turns {
        tool_round = round;

        if *ctx.cancel.borrow() {
            ctx.semantic.turn_cancelled();
            ctx.span.record("outcome", "cancelled");
            return Err(RoundError::Stop(StopReason::Cancelled));
        }

        let history_chars: usize = ctx.session.messages.iter().map(|(_, text)| text.len()).sum();
        if history_chars > COMPACTION_THRESHOLD_CHARS {
            compact_messages(&mut ctx.session.messages, EMERGENCY_COMPACTION_CHARS);
        }

        let prompt = crate::prompt::build::build_prompt(ctx.session, Some(ctx.registry));
        let rx = match ctx
            .client
            .stream(&prompt, &ctx.session.model, ctx.session.think, ctx.refs)
            .await
        {
            Ok(rx) => rx,
            Err(error) => {
                let note = actionable_error_message(&error);
                let is_overflow = error.to_string().contains("context")
                    || error.to_string().contains("too long")
                    || error.to_string().contains("tokens");

                if is_overflow && overflow_retry_count < 1 {
                    compact_messages(&mut ctx.session.messages, EMERGENCY_COMPACTION_CHARS);
                    overflow_retry_count += 1;
                    continue;
                }

                if is_overflow {
                    emit_error_chunk(
                        ctx.cx,
                        ctx.session_id,
                        ctx.message_id,
                        &format!("Context overflow persisted after emergency compaction: {error:#}"),
                    );
                    ctx.semantic.turn_failed();
                    ctx.span.record("outcome", "refusal_start");
                    return Err(RoundError::Stop(StopReason::MaxTokens));
                }

                emit_error_chunk(ctx.cx, ctx.session_id, ctx.message_id, &note);
                ctx.semantic.turn_failed();
                ctx.span.record("outcome", "failed_start");
                return Err(RoundError::Stop(StopReason::EndTurn));
            }
        };

        let is_thinking_model = gemini_acp_config::core::models::resolve(
            &ctx.session.model,
            gemini_acp_config::core::models::DEFAULT_MODEL,
        )
        .map(|resolved| gemini_acp_config::core::models::is_thinking_mode(resolved.mode))
        .unwrap_or(false);

        let streamed = stream::consume(
            rx,
            ctx.cancel,
            ctx.cx,
            ctx.session_id,
            ctx.message_id,
            is_thinking_model,
            ctx.semantic,
        )
        .await
        .map_err(RoundError::Acp)?;
        let stream::StreamResult {
            outcome,
            assistant,
            tool_detection_text,
        } = streamed;

        if matches!(outcome, stream::StreamOutcome::Cancelled) {
            ctx.semantic.turn_cancelled();
            ctx.span.record("outcome", "cancelled");
            return Err(RoundError::Stop(StopReason::Cancelled));
        }

        if let stream::StreamOutcome::Failed(error) = &outcome {
            ctx.semantic.turn_failed();
            ctx.span.record("outcome", "failed");
            return Err(RoundError::Stop(map_stop_reason_from_error(error)));
        }

        let clean_text = replace_components(&assistant);
        let (_, tool_calls) = parse_tool_calls(&tool_detection_text);
        if tool_calls.is_empty() || !ctx.session.tools_enabled || !ctx.registry.has_tools() {
            total_output = clean_text;
            break;
        }

        tracing::info!(
            session = %ctx.session_id,
            round,
            tool_count = tool_calls.len(),
            "tool calls détectés — exécution via ToolExecutor"
        );

        let tool_blocks = tool_calls
            .iter()
            .map(|call| call.to_history_block())
            .collect::<Vec<_>>()
            .join("\n");
        let assistant_history = if clean_text.is_empty() {
            tool_blocks
        } else {
            format!("{clean_text}\n{tool_blocks}")
        };
        ctx.session
            .messages
            .push((Role::Assistant, assistant_history));
        clear_partial_output(ctx.sid);

        let session_mode = ctx.session.mode;
        let mode_getter = || session_mode;
        let executor = ToolExecutor::new(
            ctx.cx,
            ctx.session_id,
            ctx.registry,
            &ctx.session.cwd,
            &ctx.session.additional_directories,
            &mode_getter,
        );
        let mut follow_up_seen = false;
        let mut follow_up_selected = None;

        for call in &tool_calls {
            if *ctx.cancel.borrow() {
                ctx.semantic.turn_cancelled();
                return Err(RoundError::Stop(StopReason::Cancelled));
            }

            if call.name == "FollowUp" {
                follow_up_seen = true;
                let label = call
                    .arguments
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Suggested next step")
                    .trim();
                let query = call
                    .arguments
                    .get("query")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !label.is_empty() && !query.is_empty() {
                    match request_action(ctx.cx, ctx.session_id, label, query).await {
                        Ok(selected) => follow_up_selected = selected,
                        Err(error) => emit_error_chunk(
                            ctx.cx,
                            ctx.session_id,
                            ctx.message_id,
                            &format!("FollowUp interaction failed: {error}"),
                        ),
                    }
                }
                break;
            }

            let result = executor
                .execute_with_call_id_and_events(
                    call.id.clone().into(),
                    &call.name,
                    &call.arguments,
                    ctx.semantic,
                )
                .await;
            ctx.session.messages.push((
                Role::Tool,
                gemini_acp_runtime::tools::prompt::format_tool_result(
                    &call.name,
                    &result.content,
                ),
            ));
        }

        if follow_up_seen {
            if let Some(query) = follow_up_selected {
                ctx.session.messages.push((Role::User, query));
                total_output.clear();
                continue;
            }
            total_output = clean_text;
            break;
        }

        if round == max_turns - 1 {
            total_output = "[Limite d'itérations outil atteinte]".into();
            assistant_already_persisted = true;
            break;
        }
    }

    Ok(RoundOutcome {
        output: total_output,
        tool_round,
        assistant_already_persisted,
    })
}
