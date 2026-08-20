mod parsers;
mod protocol;
mod reasoning;
mod types;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use agent_runtime::ModelEvent;
use crate::core::frames::GeminiFrameEvent;

use self::protocol::ProtocolDetector;
use self::reasoning::ReasoningDetector;
use self::types::{ProtocolEvent, FOLLOW_UP_PREFIX, FUNCTION_CALL_FENCE, TOOL_CALL_FENCE, TOOL_CALL_INLINE, TOOL_CALL_SINGLE_QUOTE_FENCE};

/// Normalizes Gemini frame-level events into provider-neutral `ModelEvent`s.
///
/// Structured tool calls bypass text marker parsing. Text still goes through the
/// protocol/reasoning detectors as a compatibility fallback for Gemini Web's
/// textual tool-call dialects. Duplicate tool-call ids are suppressed here so
/// execution remains at-most-once even if an upstream frame is replayed.
#[derive(Debug)]
pub struct GeminiSemanticStream {
    reasoning: ReasoningDetector,
    protocol: ProtocolDetector,
    emitted_tool_ids: HashSet<String>,
    completed: bool,
}

impl GeminiSemanticStream {
    pub fn new(supports_reasoning: bool) -> Self {
        Self {
            reasoning: ReasoningDetector::new(supports_reasoning),
            protocol: ProtocolDetector::default(),
            emitted_tool_ids: HashSet::new(),
            completed: false,
        }
    }

    pub fn feed(&mut self, frame: GeminiFrameEvent) -> Vec<ModelEvent> {
        if self.completed {
            return Vec::new();
        }
        match frame {
            GeminiFrameEvent::Text(text) => self.feed_text(&text),
            GeminiFrameEvent::ToolCall { id, name, arguments } => self.project_tool_call(id, name, arguments),
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
            ProtocolEvent::Text(text) => self.project_protocol_text(text),
            ProtocolEvent::ToolCall(call) => self.project_tool_call(call.id, call.name, call.arguments),
        }
    }

    /// A protocol detector should never return a tool-call envelope as assistant text.
    ///
    /// This second, stateless pass is intentionally defensive: it protects the typed
    /// `ModelEvent` boundary if an upstream chunk boundary or detector state ever lets a
    /// known tool marker survive the streaming parser. The runtime must not receive a
    /// hybrid `TextDelta("```tool_call ...")` event.
    fn project_protocol_text(&mut self, text: String) -> Vec<ModelEvent> {
        if !contains_protocol_marker(&text) {
            return self.reasoning.feed(text);
        }

        let mut detector = ProtocolDetector::default();
        let mut protocol_events = detector.feed(&text);
        protocol_events.extend(detector.finish());

        let mut output = Vec::new();
        for event in protocol_events {
            match event {
                ProtocolEvent::ToolCall(call) => {
                    output.extend(self.project_tool_call(call.id, call.name, call.arguments));
                }
                ProtocolEvent::Text(residual) => {
                    if contains_protocol_marker(&residual) {
                        tracing::warn!("dropping residual tool protocol that escaped semantic parsing");
                    } else if !residual.is_empty() {
                        output.extend(self.reasoning.feed(residual));
                    }
                }
            }
        }
        output
    }

    fn project_tool_call(
        &mut self,
        id: String,
        name: String,
        arguments: serde_json::Value,
    ) -> Vec<ModelEvent> {
        let id = id.trim().to_owned();
        let name = name.trim().to_owned();
        if id.is_empty() || name.is_empty() || !arguments.is_object() {
            tracing::warn!(id = %id, name = %name, "dropping malformed semantic tool call");
            return Vec::new();
        }
        if !self.emitted_tool_ids.insert(id.clone()) {
            tracing::debug!(%id, "suppressing duplicate semantic tool call");
            return Vec::new();
        }
        vec![ModelEvent::ToolCall { id, name, arguments }]
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

fn contains_protocol_marker(text: &str) -> bool {
    [
        TOOL_CALL_FENCE,
        TOOL_CALL_SINGLE_QUOTE_FENCE,
        FUNCTION_CALL_FENCE,
        TOOL_CALL_INLINE,
        FOLLOW_UP_PREFIX,
    ]
    .into_iter()
    .any(|marker| text.contains(marker))
}
