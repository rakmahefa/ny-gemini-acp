//! Gemini implementation of the provider-neutral runtime LLM contract.
use std::sync::Arc;

use crate::client::{Client, Config};
use crate::config::AgentConfig;
use agent_runtime::{LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelEvent, ModelRequest};
use tokio::sync::mpsc;

const REASONING_OPEN_MARKERS: [&str; 4] = ["<thinking>", "<think>", "[Thinking]:", "[thinking]:"];
const REASONING_CLOSE_MARKERS: [&str; 2] = ["</thinking>", "</think>"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReasoningPhase {
    Detecting,
    Response,
    Reasoning,
    Completed,
}

#[derive(Debug)]
struct GeminiSemanticStream {
    phase: ReasoningPhase,
    pending: String,
}

impl GeminiSemanticStream {
    fn new(supports_reasoning: bool) -> Self {
        Self {
            phase: if supports_reasoning { ReasoningPhase::Detecting } else { ReasoningPhase::Response },
            pending: String::new(),
        }
    }

    fn feed(&mut self, delta: &str) -> Vec<ModelEvent> {
        if delta.is_empty() || self.phase == ReasoningPhase::Completed {
            return Vec::new();
        }
        match self.phase {
            ReasoningPhase::Response => vec![ModelEvent::TextDelta(delta.to_owned())],
            ReasoningPhase::Detecting => self.feed_detecting(delta),
            ReasoningPhase::Reasoning => self.feed_reasoning(delta),
            ReasoningPhase::Completed => Vec::new(),
        }
    }

    fn finish(&mut self) -> Vec<ModelEvent> {
        if self.phase == ReasoningPhase::Completed {
            return Vec::new();
        }
        let pending = std::mem::take(&mut self.pending);
        let mut events = Vec::new();
        match self.phase {
            ReasoningPhase::Detecting | ReasoningPhase::Response => {
                if !pending.is_empty() {
                    events.push(ModelEvent::TextDelta(pending));
                }
            }
            ReasoningPhase::Reasoning => {
                if !pending.is_empty() {
                    events.push(ModelEvent::ReasoningDelta(pending));
                }
            }
            ReasoningPhase::Completed => {}
        }
        self.phase = ReasoningPhase::Completed;
        events
    }

    fn feed_detecting(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.pending.push_str(delta);
        if let Some(marker_len) = matching_marker_len(&self.pending, &REASONING_OPEN_MARKERS) {
            self.pending.drain(..marker_len);
            self.phase = ReasoningPhase::Reasoning;
            return self.feed_reasoning("");
        }
        let keep = partial_suffix_for_markers(&self.pending, &REASONING_OPEN_MARKERS);
        if self.pending.len() <= keep {
            return Vec::new();
        }
        let split_at = self.pending.len() - keep;
        let response = self.pending[..split_at].to_owned();
        self.pending.drain(..split_at);
        self.phase = ReasoningPhase::Response;
        if response.is_empty() { Vec::new() } else { vec![ModelEvent::TextDelta(response)] }
    }

    fn feed_reasoning(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.pending.push_str(delta);
        if let Some((idx, marker_len)) = find_marker(&self.pending, &REASONING_CLOSE_MARKERS) {
            let reasoning = self.pending[..idx].to_owned();
            let response = self.pending[idx + marker_len..].to_owned();
            self.pending.clear();
            self.phase = ReasoningPhase::Response;
            let mut events = Vec::with_capacity(2);
            if !reasoning.is_empty() { events.push(ModelEvent::ReasoningDelta(reasoning)); }
            if !response.is_empty() { events.push(ModelEvent::TextDelta(response)); }
            return events;
        }
        let keep = partial_suffix_for_markers(&self.pending, &REASONING_CLOSE_MARKERS);
        if self.pending.len() <= keep { return Vec::new(); }
        let split_at = self.pending.len() - keep;
        let reasoning = self.pending[..split_at].to_owned();
        self.pending.drain(..split_at);
        if reasoning.is_empty() { Vec::new() } else { vec![ModelEvent::ReasoningDelta(reasoning)] }
    }
}

fn matching_marker_len(buffer: &str, markers: &[&str]) -> Option<usize> {
    markers.iter().find(|marker| buffer.starts_with(**marker)).map(|marker| marker.len())
}

fn partial_suffix_for_markers(text: &str, markers: &[&str]) -> usize {
    markers.iter().map(|marker| partial_suffix_len(text, marker)).max().unwrap_or(0)
}

fn partial_suffix_len(text: &str, marker: &str) -> usize {
    let max = text.len().min(marker.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&marker[..len]) { return len; }
    }
    0
}

fn find_marker(buffer: &str, markers: &[&str]) -> Option<(usize, usize)> {
    markers.iter().filter_map(|marker| buffer.find(marker).map(|idx| (idx, marker.len()))).min_by_key(|(idx, _)| *idx)
}

#[derive(Clone)]
pub struct GeminiProvider {
    client: Arc<Client>,
}

impl GeminiProvider {
    pub async fn from_agent_config(config: &AgentConfig) -> anyhow::Result<Self> {
        let client_config = Config {
            cookie_file: config.cookie_file.clone(),
            default_model: config.default_model.clone(),
            auth_user: config.auth_user,
            proxy: config.proxy.clone(),
            ..Default::default()
        };
        Ok(Self { client: Arc::new(Client::new(client_config).await?) })
    }

    pub fn client(&self) -> Arc<Client> { Arc::clone(&self.client) }
}

#[async_trait::async_trait]
impl LlmProvider for GeminiProvider {
    async fn stream(&self, request: ModelRequest) -> Result<LlmStream, LlmError> {
        let upstream = self.client.stream(&request.prompt, &request.model, request.generation.reasoning_budget, &request.references).await
            .map_err(|error| LlmError::Provider(format!("{error:#}")))?;
        let (tx, rx) = mpsc::channel(16);
        let supports_reasoning = self.model_info(&request.model).supports_reasoning;
        tokio::spawn(async move {
            let mut semantic = GeminiSemanticStream::new(supports_reasoning);
            let mut upstream = upstream;
            while let Some(item) = upstream.recv().await {
                match item {
                    Ok(delta) => {
                        for event in semantic.feed(&delta) {
                            if tx.send(Ok(event)).await.is_err() { return; }
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(LlmError::Provider(error))).await;
                        return;
                    }
                }
            }
            for event in semantic.finish() {
                if tx.send(Ok(event)).await.is_err() { return; }
            }
        });
        Ok(rx)
    }

    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, LlmError> {
        self.client.upload_image(base64, mime).await.map_err(|error| LlmError::Provider(format!("{error:#}")))
    }

    fn model_info(&self, model: &str) -> LlmModelInfo {
        let supports_reasoning = crate::core::models::resolve(model, crate::core::models::DEFAULT_MODEL)
            .map(|resolved| crate::core::models::is_thinking_mode(resolved.mode)).unwrap_or(false);
        LlmModelInfo { supports_reasoning }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(stream: &mut GeminiSemanticStream, chunks: &[&str]) -> Vec<ModelEvent> {
        let mut output = Vec::new();
        for chunk in chunks { output.extend(stream.feed(chunk)); }
        output.extend(stream.finish());
        output
    }

    #[test]
    fn non_reasoning_models_pass_text_through_unchanged() {
        let mut stream = GeminiSemanticStream::new(false);
        assert_eq!(events(&mut stream, &["<thinking>hidden</thinking>answer"]), vec![ModelEvent::TextDelta("<thinking>hidden</thinking>answer".into())]);
    }

    #[test]
    fn reasoning_envelope_becomes_semantic_events() {
        let mut stream = GeminiSemanticStream::new(true);
        assert_eq!(events(&mut stream, &["<thinking>raisonnement", " utile</thinking>", "réponse"]), vec![
            ModelEvent::ReasoningDelta("raisonnement".into()),
            ModelEvent::ReasoningDelta(" utile".into()),
            ModelEvent::TextDelta("réponse".into()),
        ]);
    }

    #[test]
    fn reasoning_open_marker_split_across_chunks_is_atomic() {
        let mut stream = GeminiSemanticStream::new(true);
        assert!(stream.feed("<thi").is_empty());
        assert_eq!(stream.feed("nking>pensée</thinking>réponse"), vec![ModelEvent::ReasoningDelta("pensée".into()), ModelEvent::TextDelta("réponse".into())]);
    }

    #[test]
    fn reasoning_close_marker_split_across_chunks_is_atomic() {
        let mut stream = GeminiSemanticStream::new(true);
        assert_eq!(stream.feed("<thinking>pensée utile </thi"), vec![ModelEvent::ReasoningDelta("pensée utile ".into())]);
        assert_eq!(stream.feed("nking>réponse"), vec![ModelEvent::TextDelta("réponse".into())]);
    }

    #[test]
    fn non_enveloped_reasoning_model_response_stays_text() {
        let mut stream = GeminiSemanticStream::new(true);
        assert_eq!(events(&mut stream, &["réponse finale", " avec détails"]), vec![ModelEvent::TextDelta("réponse finale".into()), ModelEvent::TextDelta(" avec détails".into())]);
    }
}
