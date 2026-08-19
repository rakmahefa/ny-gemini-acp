mod parsers;
mod protocol;
mod reasoning;
mod types;

#[cfg(test)]
mod tests;

use agent_runtime::ModelEvent;
use crate::core::frames::GeminiFrameEvent;

use self::protocol::ProtocolDetector;
use self::reasoning::ReasoningDetector;
use self::types::ProtocolEvent;

/// Normalizes Gemini frame-level events into provider-neutral `ModelEvent`s.
///
/// Structured tool calls bypass text marker parsing. Text still goes through the
/// protocol/reasoning detectors as a compatibility fallback for Gemini Web's
/// textual tool-call dialects.
#[derive(Debug)]
pub struct GeminiSemanticStream {
    reasoning: ReasoningDetector,
    protocol: ProtocolDetector,
    completed: bool,
}

impl GeminiSemanticStream {
    pub fn new(supports_reasoning: bool) -> Self {
        Self {
            reasoning: ReasoningDetector::new(supports_reasoning),
            protocol: ProtocolDetector::default(),
            completed: false,
        }
    }

    pub fn feed(&mut self, frame: GeminiFrameEvent) -> Vec<ModelEvent> {
        if self.completed {
            return Vec::new();
        }
        match frame {
            GeminiFrameEvent::Text(text) => self.feed_text(&text),
            GeminiFrameEvent::ToolCall { id, name, arguments } => {
                vec![ModelEvent::ToolCall { id, name, arguments }]
            }
            GeminiFrameEvent::Metadata { kind, value } => self.project_metadata(&kind, value),
        }
    }

    /// Text-only compatibility entrypoint used by parser tests and by the
    /// legacy marker protocol. New production paths should prefer `feed`.
    pub fn feed_text(&mut self, text: &str) -> Vec<ModelEvent> {
        if text.is_empty() || self.completed {
            return Vec::new();
        }
        self.protocol
            .feed(text)
            .into_iter()
            .flat_map(|event| self.project_protocol_event(event))
            .collect()
    }

    pub fn finish(&mut self) -> Vec<ModelEvent> {
        if self.completed {
            return Vec::new();
        }

        let mut output = Vec::new();
        for event in self.protocol.finish() {
            output.extend(self.project_protocol_event(event));
        }
        output.extend(self.reasoning.finish());
        self.completed = true;
        output
    }

    fn project_protocol_event(&mut self, event: ProtocolEvent) -> Vec<ModelEvent> {
        match event {
            ProtocolEvent::Text(text) => self.reasoning.feed(text),
            ProtocolEvent::ToolCall(call) => vec![ModelEvent::ToolCall {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
            }],
        }
    }

    fn project_metadata(&self, kind: &str, value: serde_json::Value) -> Vec<ModelEvent> {
        if kind != "usageMetadata" && kind != "usage" {
            return Vec::new();
        }
        let Some(map) = value.as_object() else {
            return Vec::new();
        };
        let get = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| map.get(*key).and_then(serde_json::Value::as_u64))
        };
        vec![ModelEvent::Usage {
            prompt_tokens: get(&["promptTokenCount", "prompt_tokens"]),
            completion_tokens: get(&["candidatesTokenCount", "completion_tokens"]),
            total_tokens: get(&["totalTokenCount", "total_tokens"]),
        }]
    }
}
