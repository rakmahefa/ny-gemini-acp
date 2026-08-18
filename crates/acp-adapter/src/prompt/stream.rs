//! Provider stream -> ACP output orchestration.
//!
//! Le provider produit un flux neutre. Ce module reste responsable de la
//! projection sémantique vers ACP : pensée, texte visible, tool calls et
//! intégrité de stream.

use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use gemini_acp_llm::LlmStream;
use tokio::sync::watch;

use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::tools::executor::emit_error_chunk;
use gemini_acp_runtime::tools::parse::ParsedToolCall;

use super::{
    error::actionable_stream_error,
    follow_up::StreamNormalizer,
    interaction::InteractionGroup,
    notify::notify_text,
    stream_contract::{SemanticStreamContract, StreamDelta},
};

pub enum StreamOutcome {
    Complete,
    Cancelled,
    Failed(String),
}

pub struct StreamResult {
    pub outcome: StreamOutcome,
    pub assistant: String,
    pub tool_calls: Vec<ParsedToolCall>,
    pub(crate) interaction_groups: Vec<InteractionGroup>,
}

pub async fn consume(
    rx: &mut LlmStream,
    cancel: &mut watch::Receiver<bool>,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    is_thinking_model: bool,
    semantic: &mut TurnEventEmitter,
) -> Result<StreamResult, AcpError> {
    let mut thought_stream = crate::thought::ThoughtStream::new(is_thinking_model);
    let mut stream_contract = SemanticStreamContract::new();
    let mut follow_up_stream = StreamNormalizer::default();
    let mut assistant = String::new();
    let mut tool_calls = Vec::new();
    let mut interaction_groups = Vec::new();
    let mut thinking_active = false;
    semantic.assistant_started();

    let outcome = loop {
        tokio::select! {
            _ = cancel.changed() => break StreamOutcome::Cancelled,
            item = rx.recv() => {
                let Some(item) = item else { break StreamOutcome::Complete };
                match item {
                    Ok(delta) => for event in thought_stream.feed(&delta) {
                        match event {
                            crate::thought::ThoughtEvent::ThoughtStart => {
                                if !thinking_active {
                                    semantic.thinking_started();
                                    thinking_active = true;
                                }
                            }
                            crate::thought::ThoughtEvent::ThoughtChunk(text) => {
                                if !thinking_active {
                                    semantic.thinking_started();
                                    thinking_active = true;
                                }
                                semantic.thinking_delta(&text);
                                crate::thought::notify_thought(cx, session_id, message_id, &text).await?;
                            }
                            crate::thought::ThoughtEvent::ThoughtEnd => {
                                if thinking_active {
                                    semantic.thinking_completed();
                                    thinking_active = false;
                                }
                            }
                            crate::thought::ThoughtEvent::ResponseChunk(text) => {
                                if thinking_active {
                                    semantic.thinking_completed();
                                    thinking_active = false;
                                }
                                let delta = handle_response_chunk(
                                    &text,
                                    &mut stream_contract,
                                    &mut follow_up_stream,
                                    &mut assistant,
                                    semantic,
                                    cx,
                                    session_id,
                                    message_id,
                                )?;
                                tool_calls.extend(delta.tool_calls);
                                interaction_groups.extend(delta.interaction_groups);
                            }
                        }
                    },
                    Err(e) => break StreamOutcome::Failed(e.to_string()),
                }
            }
        }
    };

    for event in thought_stream.finish() {
        match event {
            crate::thought::ThoughtEvent::ThoughtStart => {
                if !thinking_active {
                    semantic.thinking_started();
                    thinking_active = true;
                }
            }
            crate::thought::ThoughtEvent::ThoughtChunk(text) => {
                if !thinking_active {
                    semantic.thinking_started();
                    thinking_active = true;
                }
                semantic.thinking_delta(&text);
                crate::thought::notify_thought(cx, session_id, message_id, &text).await?;
            }
            crate::thought::ThoughtEvent::ThoughtEnd => {
                if thinking_active {
                    semantic.thinking_completed();
                    thinking_active = false;
                }
            }
            crate::thought::ThoughtEvent::ResponseChunk(text) => {
                if thinking_active {
                    semantic.thinking_completed();
                    thinking_active = false;
                }
                let delta = handle_response_chunk(
                    &text,
                    &mut stream_contract,
                    &mut follow_up_stream,
                    &mut assistant,
                    semantic,
                    cx,
                    session_id,
                    message_id,
                )?;
                tool_calls.extend(delta.tool_calls);
                interaction_groups.extend(delta.interaction_groups);
            }
        }
    }

    if thinking_active {
        semantic.thinking_completed();
    }

    let final_delta = match stream_contract.finish() {
        Ok(delta) => delta,
        Err(error) => {
            tracing::error!(%error, "semantic stream contract violated at EOF");
            emit_error_chunk(
                cx,
                session_id,
                message_id,
                "Internal stream integrity failure: protocol output was rejected.",
            );
            Default::default()
        }
    };
    tool_calls.extend(final_delta.tool_calls);
    interaction_groups.extend(final_delta.interaction_groups);
    if !final_delta.visible.is_empty() {
        assistant.push_str(&final_delta.visible);
        let safe_message = follow_up_stream.push(&final_delta.visible);
        if !safe_message.is_empty() {
            semantic.assistant_delta(&safe_message);
            notify_text(cx, session_id, message_id, safe_message)?;
        }
    }

    let follow_up_tail = follow_up_stream.finish();
    if !follow_up_tail.is_empty() {
        assistant.push_str(&follow_up_tail);
        semantic.assistant_delta(&follow_up_tail);
        notify_text(cx, session_id, message_id, follow_up_tail)?;
    }

    if !matches!(outcome, StreamOutcome::Cancelled) {
        semantic.assistant_completed();
    }
    if let StreamOutcome::Failed(error) = &outcome {
        emit_error_chunk(cx, session_id, message_id, &actionable_stream_error(error));
    }

    Ok(StreamResult {
        outcome,
        assistant,
        tool_calls,
        interaction_groups,
    })
}

fn handle_response_chunk(
    text: &str,
    stream_contract: &mut SemanticStreamContract,
    follow_up_stream: &mut StreamNormalizer,
    assistant: &mut String,
    semantic: &mut TurnEventEmitter,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
) -> Result<StreamDelta, AcpError> {
    let delta = match stream_contract.feed(text) {
        Ok(delta) => delta,
        Err(error) => {
            tracing::error!(%error, "semantic stream contract violation; dropping unsafe delta");
            emit_error_chunk(
                cx,
                session_id,
                message_id,
                "Internal stream integrity failure: unsafe protocol output was rejected.",
            );
            return Ok(StreamDelta::default());
        }
    };

    if !delta.visible.is_empty() {
        assistant.push_str(&delta.visible);
        let safe_message = follow_up_stream.push(&delta.visible);
        if !safe_message.is_empty() {
            semantic.assistant_delta(&safe_message);
            notify_text(cx, session_id, message_id, safe_message)?;
        }
    }
    Ok(delta)
}
