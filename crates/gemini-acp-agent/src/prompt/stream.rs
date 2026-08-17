//! Streaming Gemini -> ACP output orchestration.
//!
//! This module owns semantic stream lifecycle emission. `turn.rs` only
//! coordinates the turn and consumes the normalized stream result.

use std::fmt::Display;
use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};
use tokio::sync::{mpsc, watch};

use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::tools::executor::emit_error_chunk;
use gemini_acp_runtime::tools::parse::ParsedToolCall;

use super::{
    error::actionable_stream_error,
    follow_up::StreamNormalizer,
    notify::notify_text,
    protocol_filter::ProtocolFilter,
    tool_stream::ToolStreamDetector,
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
}

pub async fn consume<E: Display>(
    mut rx: mpsc::Receiver<Result<String, E>>,
    cancel: &mut watch::Receiver<bool>,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    is_thinking_model: bool,
    semantic: &mut TurnEventEmitter,
) -> Result<StreamResult, AcpError> {
    let mut thought_stream = crate::thought::ThoughtStream::new(is_thinking_model);
    let mut protocol_filter = ProtocolFilter::new();
    let mut tool_detector = ToolStreamDetector::new();
    let mut follow_up_stream = StreamNormalizer::default();
    let mut assistant = String::new();
    let mut tool_calls = Vec::new();
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
                                tool_calls.extend(handle_response_chunk(
                                    &text,
                                    &mut tool_detector,
                                    &mut protocol_filter,
                                    &mut follow_up_stream,
                                    &mut assistant,
                                    semantic,
                                    cx,
                                    session_id,
                                    message_id,
                                )?);
                            }
                        }
                    },
                    Err(e) => break StreamOutcome::Failed(e.to_string()),
                }
            }
        }
    };

    drop(rx);
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
                tool_calls.extend(handle_response_chunk(
                    &text,
                    &mut tool_detector,
                    &mut protocol_filter,
                    &mut follow_up_stream,
                    &mut assistant,
                    semantic,
                    cx,
                    session_id,
                    message_id,
                )?);
            }
        }
    }

    if thinking_active {
        semantic.thinking_completed();
    }

    tool_calls.extend(tool_detector.finish());

    let filtered_tail = protocol_filter.finish();
    if !filtered_tail.is_empty() {
        assistant.push_str(&filtered_tail);
        let safe_message = follow_up_stream.push(&filtered_tail);
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
    })
}

fn handle_response_chunk(
    text: &str,
    tool_detector: &mut ToolStreamDetector,
    protocol_filter: &mut ProtocolFilter,
    follow_up_stream: &mut StreamNormalizer,
    assistant: &mut String,
    semantic: &mut TurnEventEmitter,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
) -> Result<Vec<ParsedToolCall>, AcpError> {
    let tool_calls = tool_detector.feed(text);
    let filtered = protocol_filter.push(text);
    if !filtered.is_empty() {
        assistant.push_str(&filtered);
        let safe_message = follow_up_stream.push(&filtered);
        if !safe_message.is_empty() {
            semantic.assistant_delta(&safe_message);
            notify_text(cx, session_id, message_id, safe_message)?;
        }
    }
    Ok(tool_calls)
}
