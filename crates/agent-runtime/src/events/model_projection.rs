use tokio::sync::mpsc;

use crate::events::TurnEventEmitter;
use crate::{LlmError, ModelEvent};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ModelProjectionError {
    #[error("model stream cancelled")]
    Cancelled,
    #[error("LLM provider failed: {0}")]
    Llm(#[source] LlmError),
    #[error("invalid model event sequence: {0}")]
    InvalidSequence(String),
    #[error("semantic event emission was rejected")]
    SemanticEventRejected,
}

#[derive(Debug)]
pub(crate) struct PendingToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: serde_json::Value,
}

#[derive(Debug)]
pub(crate) struct ModelRound {
    pub(crate) text: String,
    pub(crate) tool_calls: Vec<PendingToolCall>,
    pub(crate) event_count: usize,
}

/// Projects provider-neutral model events into runtime semantic assistant/thinking
/// lifecycle events while accumulating the model round result for orchestration.
///
/// `AgentLoop` deliberately owns turn/tool orchestration; this component owns the
/// assistant/thinking scope transitions induced by `ModelEvent`.
pub(crate) async fn consume_model_stream(
    mut stream: mpsc::Receiver<Result<ModelEvent, LlmError>>,
    cancellation: &crate::Cancellation,
    emitter: &mut TurnEventEmitter,
) -> Result<ModelRound, ModelProjectionError> {
    let mut cancel_rx = cancellation.subscribe();
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut event_count = 0usize;
    let mut assistant_active = false;
    let mut thinking_active = false;
    let mut text_started = false;

    loop {
        let item = tokio::select! {
            item = stream.recv() => item,
            changed = cancel_rx.changed() => {
                if changed.is_ok() && *cancel_rx.borrow() {
                    close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                    return Err(ModelProjectionError::Cancelled);
                }
                continue;
            }
        };

        let Some(item) = item else {
            break;
        };

        let event = match item {
            Ok(event) => event,
            Err(error) => {
                close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                return Err(ModelProjectionError::Llm(error));
            }
        };
        event_count = event_count.saturating_add(1);

        match event {
            ModelEvent::ReasoningDelta(delta) => {
                if text_started {
                    close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                    return Err(ModelProjectionError::InvalidSequence(
                        "reasoning resumed after assistant text started".into(),
                    ));
                }
                if !assistant_active {
                    if !emitter.assistant_started() {
                        return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                    }
                    assistant_active = true;
                }
                if !thinking_active {
                    if !emitter.thinking_started() {
                        return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                    }
                    thinking_active = true;
                }
                if !emitter.thinking_delta(delta) {
                    return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                }
            }
            ModelEvent::TextDelta(delta) => {
                if !assistant_active {
                    if !emitter.assistant_started() {
                        return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                    }
                    assistant_active = true;
                }
                if thinking_active {
                    if !emitter.thinking_completed() {
                        return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                    }
                    thinking_active = false;
                }
                if !delta.is_empty() {
                    text_started = true;
                }
                if !emitter.assistant_delta(delta.clone()) {
                    return Err(rejected(emitter, &mut thinking_active, &mut assistant_active));
                }
                text.push_str(&delta);
            }
            ModelEvent::ToolCall { id, name, arguments } => {
                tool_calls.push(PendingToolCall { id, name, arguments });
            }
            ModelEvent::Usage { .. } => {}
        }
    }

    if thinking_active && !emitter.thinking_completed() {
        return Err(ModelProjectionError::SemanticEventRejected);
    }
    if assistant_active && !emitter.assistant_completed() {
        return Err(ModelProjectionError::SemanticEventRejected);
    }

    Ok(ModelRound {
        text,
        tool_calls,
        event_count,
    })
}

fn rejected(
    emitter: &mut TurnEventEmitter,
    thinking_active: &mut bool,
    assistant_active: &mut bool,
) -> ModelProjectionError {
    close_active_scopes(emitter, thinking_active, assistant_active);
    ModelProjectionError::SemanticEventRejected
}

fn close_active_scopes(
    emitter: &mut TurnEventEmitter,
    thinking_active: &mut bool,
    assistant_active: &mut bool,
) {
    if *thinking_active {
        let _ = emitter.thinking_completed();
        *thinking_active = false;
    }
    if *assistant_active {
        let _ = emitter.assistant_completed();
        *assistant_active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use crate::Cancellation;

    fn emitter() -> TurnEventEmitter {
        let bus = EventBus::new();
        let mut emitter = TurnEventEmitter::new(
            bus,
            "sess_0123456789abcdef0123456789abcdef",
            "turn_test",
        );
        assert!(emitter.turn_started());
        emitter
    }

    #[tokio::test]
    async fn projects_reasoning_then_text_without_agent_loop() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ReasoningDelta("think".into()))).await.unwrap();
        tx.send(Ok(ModelEvent::TextDelta("answer".into()))).await.unwrap();
        drop(tx);

        let mut emitter = emitter();
        let round = consume_model_stream(rx, &Cancellation::new(), &mut emitter).await.unwrap();

        assert_eq!(round.text, "answer");
        assert_eq!(round.event_count, 2);
    }

    #[tokio::test]
    async fn captures_tool_calls_without_projecting_them_as_assistant_text() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ToolCall {
            id: "call-1".into(),
            name: "search".into(),
            arguments: serde_json::json!({"q": "rust"}),
        })).await.unwrap();
        drop(tx);

        let mut emitter = emitter();
        let round = consume_model_stream(rx, &Cancellation::new(), &mut emitter).await.unwrap();

        assert_eq!(round.event_count, 1);
        assert_eq!(round.text, "");
        assert_eq!(round.tool_calls.len(), 1);
        assert_eq!(round.tool_calls[0].id, "call-1");
        assert_eq!(round.tool_calls[0].name, "search");
    }

    #[tokio::test]
    async fn rejects_reasoning_after_text() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::TextDelta("answer".into()))).await.unwrap();
        tx.send(Ok(ModelEvent::ReasoningDelta("late".into()))).await.unwrap();
        drop(tx);

        let mut emitter = emitter();
        let error = consume_model_stream(rx, &Cancellation::new(), &mut emitter).await.unwrap_err();

        assert!(matches!(error, ModelProjectionError::InvalidSequence(message) if message.contains("reasoning resumed")));
    }

    #[tokio::test]
    async fn closes_scopes_before_returning_provider_error() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ReasoningDelta("think".into()))).await.unwrap();
        tx.send(Err(LlmError::Provider("boom".into()))).await.unwrap();
        drop(tx);

        let mut emitter = emitter();
        let error = consume_model_stream(rx, &Cancellation::new(), &mut emitter).await.unwrap_err();

        assert!(matches!(error, ModelProjectionError::Llm(_)));
    }
}
