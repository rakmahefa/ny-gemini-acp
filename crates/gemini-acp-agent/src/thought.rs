//! Encapsulation du flux `thinking` Gemini → ACP.
//!
//! Le parseur est indépendant du transport Gemini et de la couche ACP. Il
//! transforme un flux de deltas en événements sémantiques : pensée explicite,
//! transition vers la réponse, ou réponse normale.
//!
//! Important : un modèle configuré en mode thinking n'implique pas que le
//! flux texte du transport contienne effectivement un bloc de pensée. Le
//! transport Gemini Web peut exposer uniquement la réponse finale. Dans ce
//! cas, nous ne devons jamais classer arbitrairement la réponse comme pensée :
//! elle reste une réponse assistant valide.

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
    TextContent,
};
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError};

const MARKER_LOOKBEHIND: usize = 32;
const THINKING_OPEN_MARKERS: [&str; 4] = ["<thinking>", "<think>", "[Thinking]:", "[thinking]:"];
const THINKING_CLOSE_MARKERS: [&str; 2] = ["</thinking>", "</think>"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThoughtPhase {
    /// A thinking-capable model is being probed for an explicit thought block.
    Detecting,
    /// The response is normal assistant content; no explicit thought block was present.
    Response,
    /// An explicit thought block is being streamed.
    Thinking,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThoughtEvent {
    ThoughtStart,
    ThoughtChunk(String),
    ThoughtEnd,
    ResponseChunk(String),
}

#[derive(Debug)]
pub struct ThoughtStream {
    phase: ThoughtPhase,
    pending: String,
    emitted_thought: bool,
}

impl ThoughtStream {
    pub fn new(is_thinking_model: bool) -> Self {
        Self {
            phase: if is_thinking_model {
                ThoughtPhase::Detecting
            } else {
                ThoughtPhase::Response
            },
            pending: String::new(),
            emitted_thought: false,
        }
    }

    pub fn phase(&self) -> ThoughtPhase {
        self.phase
    }

    pub fn has_emitted_thought(&self) -> bool {
        self.emitted_thought
    }

    pub fn feed(&mut self, delta: &str) -> Vec<ThoughtEvent> {
        if delta.is_empty() || self.phase == ThoughtPhase::Completed {
            return Vec::new();
        }

        match self.phase {
            ThoughtPhase::Response => vec![ThoughtEvent::ResponseChunk(delta.to_owned())],
            ThoughtPhase::Detecting => self.feed_detecting(delta),
            ThoughtPhase::Thinking => self.feed_thinking(delta, false),
            ThoughtPhase::Completed => Vec::new(),
        }
    }

    pub fn finish(&mut self) -> Vec<ThoughtEvent> {
        if self.phase == ThoughtPhase::Completed {
            return Vec::new();
        }

        let mut events = Vec::new();
        let pending = std::mem::take(&mut self.pending);

        match self.phase {
            ThoughtPhase::Detecting => {
                // No explicit thought envelope arrived. Treat the buffered
                // payload as assistant response rather than silently dropping
                // it or misclassifying it as hidden reasoning.
                if !pending.is_empty() {
                    events.push(ThoughtEvent::ResponseChunk(pending));
                }
            }
            ThoughtPhase::Response => {
                if !pending.is_empty() {
                    events.push(ThoughtEvent::ResponseChunk(pending));
                }
            }
            ThoughtPhase::Thinking => {
                if !pending.is_empty() {
                    self.emitted_thought = true;
                    events.push(ThoughtEvent::ThoughtChunk(pending));
                }
                events.push(ThoughtEvent::ThoughtEnd);
            }
            ThoughtPhase::Completed => {}
        }

        self.phase = ThoughtPhase::Completed;
        events
    }

    fn feed_detecting(&mut self, delta: &str) -> Vec<ThoughtEvent> {
        self.pending.push_str(delta);

        if let Some(marker) = matching_open_marker(&self.pending) {
            self.pending.drain(..marker.len());
            self.phase = ThoughtPhase::Thinking;
            return vec![ThoughtEvent::ThoughtStart];
        }

        if self.pending.len() < max_prefix_len(&THINKING_OPEN_MARKERS) {
            return Vec::new();
        }

        // The beginning of the stream is not an explicit thought envelope.
        // Flush all safely classified text as assistant response and switch to
        // the normal response phase for the remainder of the stream.
        let response = std::mem::take(&mut self.pending);
        self.phase = ThoughtPhase::Response;
        if response.is_empty() {
            Vec::new()
        } else {
            vec![ThoughtEvent::ResponseChunk(response)]
        }
    }

    fn feed_thinking(&mut self, delta: &str, _final_chunk: bool) -> Vec<ThoughtEvent> {
        self.pending.push_str(delta);

        if let Some((idx, marker_len)) = find_thought_end(&self.pending) {
            let thought = self.pending[..idx].to_owned();
            let message = self.pending[idx + marker_len..].to_owned();
            self.pending.clear();
            self.phase = ThoughtPhase::Response;

            let mut events = Vec::with_capacity(3);
            if !thought.is_empty() {
                self.emitted_thought = true;
                events.push(ThoughtEvent::ThoughtChunk(thought));
            }
            events.push(ThoughtEvent::ThoughtEnd);
            if !message.is_empty() {
                events.push(ThoughtEvent::ResponseChunk(message));
            }
            return events;
        }

        let char_count = self.pending.chars().count();
        if char_count > MARKER_LOOKBEHIND {
            let emit_chars = char_count - MARKER_LOOKBEHIND;
            let split_at = self
                .pending
                .char_indices()
                .nth(emit_chars)
                .map(|(idx, _)| idx)
                .unwrap_or(self.pending.len());
            let thought = self.pending[..split_at].to_owned();
            self.pending.drain(..split_at);
            if !thought.is_empty() {
                self.emitted_thought = true;
                return vec![ThoughtEvent::ThoughtChunk(thought)];
            }
        }

        Vec::new()
    }
}

/// Compatibilité temporaire pour les consommateurs existants.
///
/// La nouvelle orchestration doit consommer `ThoughtStream` directement. Ce
/// wrapper conserve toutefois le contrat historique `(thought, message)` afin
/// que la migration puisse être faite sans rupture intermédiaire.
#[derive(Debug)]
pub struct ThoughtSplitter {
    stream: ThoughtStream,
}

impl ThoughtSplitter {
    pub fn new(is_thinking_model: bool) -> Self {
        Self {
            stream: ThoughtStream::new(is_thinking_model),
        }
    }

    pub fn feed(&mut self, delta: &str) -> (String, String) {
        let mut thought = String::new();
        let mut message = String::new();
        for event in self.stream.feed(delta) {
            match event {
                ThoughtEvent::ThoughtStart | ThoughtEvent::ThoughtEnd => {}
                ThoughtEvent::ThoughtChunk(text) => thought.push_str(&text),
                ThoughtEvent::ResponseChunk(text) => message.push_str(&text),
            }
        }
        (thought, message)
    }

    pub fn flush(&mut self) -> (String, String) {
        let mut thought = String::new();
        let mut message = String::new();
        for event in self.stream.finish() {
            match event {
                ThoughtEvent::ThoughtStart | ThoughtEvent::ThoughtEnd => {}
                ThoughtEvent::ThoughtChunk(text) => thought.push_str(&text),
                ThoughtEvent::ResponseChunk(text) => message.push_str(&text),
            }
        }
        (thought, message)
    }

    pub fn has_emitted_thought(&self) -> bool {
        self.stream.has_emitted_thought()
    }
}

fn matching_open_marker(buffer: &str) -> Option<&'static str> {
    THINKING_OPEN_MARKERS
        .iter()
        .copied()
        .find(|marker| buffer.starts_with(marker))
}

fn max_prefix_len(markers: &[&str]) -> usize {
    markers.iter().map(|marker| marker.len()).max().unwrap_or(0)
}

fn find_thought_end(buffer: &str) -> Option<(usize, usize)> {
    THINKING_CLOSE_MARKERS
        .iter()
        .filter_map(|marker| buffer.find(marker).map(|idx| (idx, marker.len())))
        .min_by_key(|(idx, _)| *idx)
}

/// Compat helper used by tests and by future marker implementations.
fn partial_suffix_len(text: &str, needle: &str) -> usize {
    let max = text.len().min(needle.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&needle[..len]) {
            return len;
        }
    }
    0
}

pub async fn notify_thought(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message_id: &MessageId,
    text: &str,
) -> Result<(), AcpError> {
    if text.is_empty() {
        return Ok(());
    }

    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentThoughtChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new(text.to_owned())))
                .message_id(message_id.clone()),
        ),
    ))
}

#[cfg(test)]
#[path = "test/thought.rs"]
mod tests;
