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
use super::{error::actionable_stream_error, follow_up::StreamNormalizer, notify::notify_text, output_filter::OutputFilter};

pub enum StreamOutcome { Complete, Cancelled, Failed(String) }
pub struct StreamResult { pub outcome: StreamOutcome, pub assistant: String, pub tool_detection_text: String }

pub async fn consume<E: Display>(
    mut rx: mpsc::Receiver<Result<String, E>>,
    cancel: &mut watch::Receiver<bool>,
    cx: &ConnectionTo<Client>, session_id: &SessionId, message_id: &MessageId,
    is_thinking_model: bool, semantic: &mut TurnEventEmitter,
) -> Result<StreamResult, AcpError> {
    let mut thought_stream = crate::thought::ThoughtStream::new(is_thinking_model);
    let mut output_filter = OutputFilter::new();
    let mut follow_up_stream = StreamNormalizer::default();
    let mut assistant = String::new();
    let mut tool_detection_text = String::new();
    semantic.assistant_started();
    if is_thinking_model { semantic.thinking_started(); }
    let outcome = loop {
        tokio::select! {
            _ = cancel.changed() => break StreamOutcome::Cancelled,
            item = rx.recv() => {
                let Some(item) = item else { break StreamOutcome::Complete };
                match item {
                    Ok(delta) => for event in thought_stream.feed(&delta) {
                        match event {
                            crate::thought::ThoughtEvent::ThoughtChunk(text) => {
                                semantic.thinking_delta(&text);
                                crate::thought::notify_thought(cx, session_id, message_id, &text).await?;
                            }
                            crate::thought::ThoughtEvent::ThoughtEnd => { if is_thinking_model { semantic.thinking_completed(); } }
                            crate::thought::ThoughtEvent::ResponseChunk(text) => {
                                // Only response content is eligible for tool parsing.
                                // Thinking content is never reinterpreted as a tool call.
                                tool_detection_text.push_str(&text);
                                let filtered = output_filter.push(&text);
                                if !filtered.is_empty() {
                                    assistant.push_str(&filtered);
                                    let safe_message = follow_up_stream.push(&filtered);
                                    if !safe_message.is_empty() {
                                        semantic.assistant_delta(&safe_message);
                                        notify_text(cx, session_id, message_id, safe_message)?;
                                    }
                                }
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
            crate::thought::ThoughtEvent::ThoughtChunk(text) => {
                semantic.thinking_delta(&text);
                crate::thought::notify_thought(cx, session_id, message_id, &text).await?;
            }
            crate::thought::ThoughtEvent::ThoughtEnd => { if is_thinking_model { semantic.thinking_completed(); } }
            crate::thought::ThoughtEvent::ResponseChunk(text) => {
                tool_detection_text.push_str(&text);
                let filtered = output_filter.push(&text);
                if !filtered.is_empty() {
                    assistant.push_str(&filtered);
                    let safe_message = follow_up_stream.push(&filtered);
                    if !safe_message.is_empty() {
                        semantic.assistant_delta(&safe_message);
                        notify_text(cx, session_id, message_id, safe_message)?;
                    }
                }
            }
        }
    }
    let filtered_tail = output_filter.finish();
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
    if !matches!(outcome, StreamOutcome::Cancelled) { semantic.assistant_completed(); }
    if let StreamOutcome::Failed(error) = &outcome { emit_error_chunk(cx, session_id, message_id, &actionable_stream_error(error)); }
    Ok(StreamResult { outcome, assistant, tool_detection_text })
}
