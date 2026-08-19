mod parsers;
mod protocol;
mod reasoning;
mod types;

#[cfg(test)]
mod tests;

use agent_runtime::ModelEvent;

use self::protocol::ProtocolDetector;
use self::reasoning::ReasoningDetector;
use self::types::ProtocolEvent;

/// Incrementally normalizes Gemini's mixed text/protocol stream into provider-neutral model events.
///
/// The facade owns stream lifecycle only. Protocol framing and reasoning marker detection live in
/// dedicated modules so each parser can evolve without changing the provider-facing contract.
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

    pub fn feed(&mut self, delta: &str) -> Vec<ModelEvent> {
        if delta.is_empty() || self.completed {
            return Vec::new();
        }

        self.protocol
            .feed(delta)
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
}
