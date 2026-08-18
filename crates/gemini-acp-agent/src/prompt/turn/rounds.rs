use agent_client_protocol::schema::v1::{MessageId, SessionId, StopReason};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_llm::{LlmProvider, LlmRequest};
use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::state::{Role, Session};
use gemini_acp_runtime::tools::executor::{emit_error_chunk, ToolExecutor};
use gemini_acp_runtime::tools::ToolRegistry;
use tokio::sync::watch;

use super::context::{compact_messages, COMPACTION_THRESHOLD_CHARS, EMERGENCY_COMPACTION_CHARS};
use crate::prompt::error::actionable_error_message;
use crate::prompt::follow_up::{replace_components, request_action, FollowUpError, FollowUpOutcome};
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
    pub(crate) provider: &'a dyn LlmProvider,
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

    'rounds: for round in 0..max_turns {
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
        let request = LlmRequest {
            prompt,
            model: ctx.session.model.clone(),
            thinking: ctx.session.think,
            refs: ctx.refs.to_vec(),
        };
        let mut stream = match ctx.provider.stream(request).await {
            Ok(stream) => stream,
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
                        &format!("Context overflow persisted after emergency compaction: {error}"),
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

        let is_thinking_model = ctx.provider.is_thinking_model(&ctx.session.model);

        let streamed = stream::consume(
            &mut stream,
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
            tool_calls,
            interaction_groups,
        } = streamed;
        let _ = interaction_groups;

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
        let follow_up_calls = tool_calls.iter().filter(|call| call.is_action()).collect::<Vec<_>>();
        let executable_calls = tool_calls
            .iter()
            .filter(|call| !call.is_action())
            .collect::<Vec<_>>();

        if tool_calls.is_empty() {
            total_output = clean_text;
            break;
        }

        if !executable_calls.is_empty() && !ctx.session.tools_enabled {
            tracing::warn!(
                session = %ctx.session_id,
                action_count = follow_up_calls.len(),
                tool_count = executable_calls.len(),
                "executable tool calls were suppressed because tools are disabled"
            );
        }

        if !executable_calls.is_empty() && !ctx.registry.has_tools() {
            tracing::warn!(
                session = %ctx.session_id,
                action_count = follow_up_calls.len(),
                tool_count = executable_calls.len(),
                "executable tool calls were suppressed because the tool registry is empty"
            );
        }

        let executable_calls = if ctx.session.tools_enabled && ctx.registry.has_tools() {
            executable_calls
        } else {
            Vec::new()
        };

        if executable_calls.is_empty() && follow_up_calls.is_empty() {
            total_output = clean_text;
            break;
        }

        tracing::info!(
            session = %ctx.session_id,
            round,
            tool_count = executable_calls.len(),
            follow_up_count = follow_up_calls.len(),
            provider = ctx.provider.name(),
            "stream protocol actions normalized"
        );

        if !executable_calls.is_empty() {
            let tool_blocks = executable_calls
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

            for call in &executable_calls {
                if *ctx.cancel.borrow() {
                    ctx.semantic.turn_cancelled();
                    return Err(RoundError::Stop(StopReason::Cancelled));
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
        }

        for call in follow_up_calls {
            let label = call
                .arguments
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            let query = call
                .arguments
                .get("query")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();

            if label.is_empty() || query.is_empty() {
                tracing::warn!(
                    session = %ctx.session_id,
                    action_id = %call.id,
                    "malformed FollowUp action ignored"
                );
                continue;
            }

            match request_action(
                ctx.cx,
                ctx.session_id,
                &call.id,
                label,
                query,
                ctx.cancel,
            )
            .await
            {
                Ok(FollowUpOutcome::Selected(selected_query)) => {
                    ctx.session.messages.push((Role::User, selected_query));
                    total_output.clear();
                    continue 'rounds;
                }
                Ok(FollowUpOutcome::Rejected) => {
                    continue;
                }
                Ok(FollowUpOutcome::Cancelled) => {
                    ctx.semantic.turn_cancelled();
                    ctx.span.record("outcome", "cancelled_follow_up");
                    return Err(RoundError::Stop(StopReason::Cancelled));
                }
                Err(error) => {
                    let is_invalid_input = matches!(&error, FollowUpError::InvalidInput(_));
                    tracing::warn!(
                        session = %ctx.session_id,
                        action_id = %call.id,
                        invalid_input = is_invalid_input,
                        error = %error,
                        "FollowUp interaction rejected without corrupting the containing turn"
                    );
                    if !is_invalid_input {
                        emit_error_chunk(
                            ctx.cx,
                            ctx.session_id,
                            ctx.message_id,
                            &format!("FollowUp interaction failed: {error}"),
                        );
                    }
                }
            }
        }

        if executable_calls.is_empty() {
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
