use agent_runtime::ModelEvent;

use super::types::{ReasoningPhase, REASONING_CLOSE_MARKERS, REASONING_OPEN_MARKERS};

#[derive(Debug)]
pub(super) struct ReasoningDetector {
    phase: ReasoningPhase,
    pending: String,
}

impl ReasoningDetector {
    pub(super) fn new(supports_reasoning: bool) -> Self {
        Self {
            phase: if supports_reasoning {
                ReasoningPhase::Detecting
            } else {
                ReasoningPhase::Response
            },
            pending: String::new(),
        }
    }

    pub(super) fn feed(&mut self, delta: String) -> Vec<ModelEvent> {
        if delta.is_empty() {
            return Vec::new();
        }

        match self.phase {
            ReasoningPhase::Response => vec![ModelEvent::TextDelta(delta)],
            ReasoningPhase::Detecting => self.feed_detecting(&delta),
            ReasoningPhase::Reasoning => self.feed_body(&delta),
            ReasoningPhase::Completed => Vec::new(),
        }
    }

    pub(super) fn finish(&mut self) -> Vec<ModelEvent> {
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            self.phase = ReasoningPhase::Completed;
            return Vec::new();
        }

        let event = match self.phase {
            ReasoningPhase::Reasoning => ModelEvent::ReasoningDelta(pending),
            ReasoningPhase::Detecting | ReasoningPhase::Response => ModelEvent::TextDelta(pending),
            ReasoningPhase::Completed => return Vec::new(),
        };

        self.phase = ReasoningPhase::Completed;
        vec![event]
    }

    fn feed_detecting(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.pending.push_str(delta);

        if let Some(marker_len) = matching_marker_len(&self.pending, &REASONING_OPEN_MARKERS) {
            self.pending.drain(..marker_len);
            self.phase = ReasoningPhase::Reasoning;
            let pending = std::mem::take(&mut self.pending);
            return self.feed(pending);
        }

        let keep = partial_suffix_for_markers(&self.pending, &REASONING_OPEN_MARKERS);
        if self.pending.len() <= keep {
            return Vec::new();
        }

        let split_at = self.pending.len() - keep;
        let response = self.pending[..split_at].to_owned();
        self.pending.drain(..split_at);
        self.phase = ReasoningPhase::Response;

        if response.is_empty() {
            Vec::new()
        } else {
            vec![ModelEvent::TextDelta(response)]
        }
    }

    fn feed_body(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.pending.push_str(delta);

        if let Some((idx, marker_len)) = find_marker(&self.pending, &REASONING_CLOSE_MARKERS) {
            let reasoning = self.pending[..idx].to_owned();
            let response = self.pending[idx + marker_len..].to_owned();
            self.pending.clear();
            self.phase = ReasoningPhase::Response;

            let mut events = Vec::new();
            if !reasoning.is_empty() {
                events.push(ModelEvent::ReasoningDelta(reasoning));
            }
            if !response.is_empty() {
                events.push(ModelEvent::TextDelta(response));
            }
            return events;
        }

        let keep = partial_suffix_for_markers(&self.pending, &REASONING_CLOSE_MARKERS);
        if self.pending.len() <= keep {
            return Vec::new();
        }

        let split_at = self.pending.len() - keep;
        let reasoning = self.pending[..split_at].to_owned();
        self.pending.drain(..split_at);

        if reasoning.is_empty() {
            Vec::new()
        } else {
            vec![ModelEvent::ReasoningDelta(reasoning)]
        }
    }
}

fn matching_marker_len(buffer: &str, markers: &[&str]) -> Option<usize> {
    markers
        .iter()
        .find(|marker| buffer.starts_with(**marker))
        .map(|marker| marker.len())
}

fn partial_suffix_for_markers(text: &str, markers: &[&str]) -> usize {
    markers
        .iter()
        .map(|marker| partial_suffix_len(text, marker))
        .max()
        .unwrap_or(0)
}

fn partial_suffix_len(text: &str, marker: &str) -> usize {
    let max = text.len().min(marker.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&marker[..len]) {
            return len;
        }
    }
    0
}

fn find_marker(buffer: &str, markers: &[&str]) -> Option<(usize, usize)> {
    markers
        .iter()
        .filter_map(|marker| buffer.find(marker).map(|idx| (idx, marker.len())))
        .min_by_key(|(idx, _)| *idx)
}
