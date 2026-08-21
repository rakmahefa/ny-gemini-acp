use tokio::sync::mpsc;

use crate::events::TurnEventSink;
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

pub(crate) async fn consume_model_stream<S: TurnEventSink + ?Sized>(
    mut stream: mpsc::Receiver<Result<ModelEvent, LlmError>>,
    cancellation: &crate::Cancellation,
    sink: &mut S,
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
                    close_active_scopes(sink, &mut thinking_active, &mut assistant_active);
                    return Err(ModelProjectionError::Cancelled);
                }
                continue;
            }
        };

        let Some(item) = item else { break; };
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                close_active_scopes(sink, &mut thinking_active, &mut assistant_active);
                return Err(ModelProjectionError::Llm(error));
            }
        };
        event_count = event_count.saturating_add(1);

        match event {
            ModelEvent::ReasoningDelta(delta) => {
                if text_started {
                    close_active_scopes(sink, &mut thinking_active, &mut assistant_active);
                    return Err(ModelProjectionError::InvalidSequence("reasoning resumed after assistant text started".into()));
                }
                if !assistant_active {
                    if !sink.assistant_started() { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
                    assistant_active = true;
                }
                if !thinking_active {
                    if !sink.thinking_started() { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
                    thinking_active = true;
                }
                if !sink.thinking_delta(delta) { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
            }
            ModelEvent::TextDelta(delta) => {
                if !assistant_active {
                    if !sink.assistant_started() { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
                    assistant_active = true;
                }
                if thinking_active {
                    if !sink.thinking_completed() { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
                    thinking_active = false;
                }
                if !delta.is_empty() { text_started = true; }
                if !sink.assistant_delta(delta.clone()) { return Err(rejected(sink, &mut thinking_active, &mut assistant_active)); }
                text.push_str(&delta);
            }
            ModelEvent::ToolCall { id, name, arguments } => {
                tool_calls.push(PendingToolCall { id, name, arguments });
            }
            ModelEvent::Usage { .. } => {}
        }
    }

    if thinking_active && !sink.thinking_completed() { return Err(ModelProjectionError::SemanticEventRejected); }
    if assistant_active && !sink.assistant_completed() { return Err(ModelProjectionError::SemanticEventRejected); }

    Ok(ModelRound { text, tool_calls, event_count })
}

fn rejected<S: TurnEventSink + ?Sized>(sink: &mut S, thinking_active: &mut bool, assistant_active: &mut bool) -> ModelProjectionError {
    close_active_scopes(sink, thinking_active, assistant_active);
    ModelProjectionError::SemanticEventRejected
}

fn close_active_scopes<S: TurnEventSink + ?Sized>(sink: &mut S, thinking_active: &mut bool, assistant_active: &mut bool) {
    if *thinking_active { let _ = sink.thinking_completed(); *thinking_active = false; }
    if *assistant_active { let _ = sink.assistant_completed(); *assistant_active = false; }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, TurnEventEmitter};
    use crate::Cancellation;

    struct TestEmitter {
        emitter: TurnEventEmitter,
        _receiver: mpsc::UnboundedReceiver<crate::events::SemanticEvent>,
    }

    fn emitter() -> TestEmitter {
        let bus = EventBus::new();
        let receiver = bus.subscribe_turn("turn_test");
        TestEmitter {
            emitter: TurnEventEmitter::new(bus, "sess_0123456789abcdef0123456789abcdef", "turn_test"),
            _receiver: receiver,
        }
    }

    #[tokio::test]
    async fn projects_reasoning_then_text_without_agent_loop() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ReasoningDelta("think".into()))).await.unwrap();
        tx.send(Ok(ModelEvent::TextDelta("answer".into()))).await.unwrap();
        drop(tx);
        let mut harness = emitter();
        let round = consume_model_stream(rx, &Cancellation::new(), &mut harness.emitter).await.unwrap();
        assert_eq!(round.text, "answer");
        assert_eq!(round.event_count, 2);
    }

    #[tokio::test]
    async fn captures_tool_calls_without_projecting_them_as_assistant_text() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ToolCall { id: "call-1".into(), name: "search".into(), arguments: serde_json::json!({"q": "rust"}) })).await.unwrap();
        drop(tx);
        let mut harness = emitter();
        let round = consume_model_stream(rx, &Cancellation::new(), &mut harness.emitter).await.unwrap();
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
        let mut harness = emitter();
        let error = consume_model_stream(rx, &Cancellation::new(), &mut harness.emitter).await.unwrap_err();
        assert!(matches!(error, ModelProjectionError::InvalidSequence(message) if message.contains("reasoning resumed")));
    }

    #[tokio::test]
    async fn closes_scopes_before_returning_provider_error() {
        let (tx, rx) = mpsc::channel(4);
        tx.send(Ok(ModelEvent::ReasoningDelta("think".into()))).await.unwrap();
        tx.send(Err(LlmError::Provider("boom".into()))).await.unwrap();
        drop(tx);
        let mut harness = emitter();
        let error = consume_model_stream(rx, &Cancellation::new(), &mut harness.emitter).await.unwrap_err();
        assert!(matches!(error, ModelProjectionError::Llm(_)));
    }
}
