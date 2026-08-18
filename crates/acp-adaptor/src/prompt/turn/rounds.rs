use agent_client_protocol::schema::v1::{MessageId, SessionId, StopReason};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::state::{Role, Session};
use gemini_acp_runtime::{LlmProvider, LlmRequest, ToolProvider};
use tokio::sync::watch;

use super::context::{compact_messages, COMPACTION_THRESHOLD_CHARS, EMERGENCY_COMPACTION_CHARS};
use crate::prompt::follow_up::{replace_components, request_action, FollowUpError, FollowUpOutcome};
use crate::prompt::stream;

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
    pub(crate) llm: &'a dyn LlmProvider,
    pub(crate) cx: &'a ConnectionTo<Client>,
    pub(crate) session_id: &'a SessionId,
    pub(crate) sid: &'a str,
    pub(crate) message_id: &'a MessageId,
    pub(crate) cancel: &'a mut watch::Receiver<bool>,
    pub(crate) session: &'a mut Session,
    pub(crate) provider: &'a dyn ToolProvider,
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
            return Err(RoundError::Stop(StopReason::Cancelled));
        }

        let history_chars: usize = ctx.session.messages.iter().map(|(_, t)| t.len()).sum();
        if history_chars > COMPACTION_THRESHOLD_CHARS {
            compact_messages(&mut ctx.session.messages, EMERGENCY_COMPACTION_CHARS);
        }

        let prompt = crate::prompt::build::build_prompt(ctx.session, Some(ctx.provider));
        let rx = match ctx
            .llm
            .stream(LlmRequest {
                prompt,
                model: ctx.session.model.clone(),
                think: ctx.session.think,
                refs: ctx.refs.to_vec(),
            })
            .await
        {
            Ok(rx) => rx,
            Err(error) => {
                let overflow = error.to_string().contains("context")
                    || error.to_string().contains("too long")
                    || error.to_string().contains("tokens");
                if overflow && overflow_retry_count < 1 {
                    compact_messages(&mut ctx.session.messages, EMERGENCY_COMPACTION_CHARS);
                    overflow_retry_count += 1;
                    continue;
                }
                ctx.semantic.turn_failed();
                return Err(RoundError::Stop(if overflow {
                    StopReason::MaxTokens
                } else {
                    StopReason::EndTurn
                }));
            }
        };

        let thinking = ctx.llm.model_info(&ctx.session.model).supports_thinking;
        let streamed = stream::consume(
            rx,
            ctx.cancel,
            ctx.cx,
            ctx.session_id,
            ctx.message_id,
            thinking,
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
            return Err(RoundError::Stop(StopReason::Cancelled));
        }
        if let stream::StreamOutcome::Failed(error) = &outcome {
            ctx.semantic.turn_failed();
            return Err(RoundError::Stop(map_stop_reason_from_error(error)));
        }

        let clean_text = replace_components(&assistant);
        let follow_up_calls = tool_calls.iter().filter(|c| c.is_action()).collect::<Vec<_>>();
        let executable_calls = tool_calls.iter().filter(|c| !c.is_action()).collect::<Vec<_>>();

        if tool_calls.is_empty() {
            total_output = clean_text;
            break;
        }

        let executable_calls = if ctx.session.tools_enabled && ctx.provider.has_tools() {
            executable_calls
        } else {
            Vec::new()
        };

        if executable_calls.is_empty() && follow_up_calls.is_empty() {
            total_output = clean_text;
            break;
        }

        if !executable_calls.is_empty() {
            let tool_blocks = executable_calls
                .iter()
                .map(|c| c.to_history_block())
                .collect::<Vec<_>>()
                .join("\n");
            let assistant_history = if clean_text.is_empty() {
                tool_blocks
            } else {
                format!("{clean_text}\n{tool_blocks}")
            };
            ctx.session.messages.push((Role::Assistant, assistant_history));

            let session_mode = ctx.session.mode;
            let mode_getter = || session_mode.into();
            let executor = gemini_acp_tools::tools::executor::ToolExecutor::new(
                ctx.cx,
                ctx.session_id,
                ctx.provider,
                &ctx.session.cwd,
                &ctx.session.additional_directories,
                &mode_getter,
                ctx.cancel.clone(),
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
                    gemini_acp_tools::tools::prompt::format_tool_result(&call.name, &result.content),
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
                continue;
            }
            match request_action(ctx.cx, ctx.session_id, &call.id, label, query, ctx.cancel).await {
                Ok(FollowUpOutcome::Selected(q)) => {
                    ctx.session.messages.push((Role::User, q));
                    total_output.clear();
                    continue 'rounds;
                }
                Ok(FollowUpOutcome::Rejected) => continue,
                Ok(FollowUpOutcome::Cancelled) => {
                    ctx.semantic.turn_cancelled();
                    return Err(RoundError::Stop(StopReason::Cancelled));
                }
                Err(error) => {
                    if !matches!(&error, FollowUpError::InvalidInput(_)) {
                        crate::prompt::notify::emit_error_chunk(
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
