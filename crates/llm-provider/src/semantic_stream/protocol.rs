use serde_json::json;

use super::parsers::{allocate_call_id, parse_bare_json, parse_follow_up_candidates};
use super::types::{
    BlockKind, ModelToolCall, ProtocolEvent, ProtocolMode, FOLLOW_UP_PREFIX, MAX_FOLLOW_UP,
    MAX_PENDING, MAX_TOOL_BLOCK, TOOL_RESULT_ENVELOPE, TOOL_RESULT_PREFIX,
};

#[derive(Debug)]
pub(super) struct ProtocolDetector {
    mode: ProtocolMode,
    pending: String,
    at_stream_start: bool,
    next_call_id: usize,
}

impl Default for ProtocolDetector {
    fn default() -> Self {
        Self {
            mode: ProtocolMode::Normal,
            pending: String::new(),
            at_stream_start: true,
            next_call_id: 0,
        }
    }
}

impl ProtocolDetector {
    pub(super) fn feed(&mut self, chunk: &str) -> Vec<ProtocolEvent> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.pending.push_str(chunk);
        if self.pending.len() > MAX_PENDING {
            self.pending.clear();
            self.mode = ProtocolMode::Normal;
            return Vec::new();
        }
        self.drain(false)
    }

    pub(super) fn finish(&mut self) -> Vec<ProtocolEvent> {
        let events = self.drain(true);
        self.mode = ProtocolMode::Normal;
        self.pending.clear();
        events
    }

    fn drain(&mut self, final_flush: bool) -> Vec<ProtocolEvent> {
        let mut events = Vec::new();
        loop {
            let progressed = match self.mode {
                ProtocolMode::Normal => self.drain_normal(&mut events, final_flush),
                ProtocolMode::IgnoreToolResult { closing } => {
                    self.drain_tool_result(closing, final_flush)
                }
                ProtocolMode::ToolBlock { .. } => self.drain_tool_block(&mut events, final_flush),
            };
            if !progressed {
                break;
            }
        }
        events
    }

    fn drain_normal(&mut self, events: &mut Vec<ProtocolEvent>, final_flush: bool) -> bool {
        if let Some(start) = self
            .pending
            .find(TOOL_RESULT_PREFIX)
            .or_else(|| self.pending.find(TOOL_RESULT_ENVELOPE))
        {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }

            let Some(newline) = self.pending.find('\n') else {
                if final_flush {
                    self.pending.clear();
                    self.mode = ProtocolMode::Normal;
                }
                return false;
            };

            let first_line = self.pending[..newline].trim_end_matches(['\r', '\n']);
            let closing = if first_line.contains("```tool_call")
                || first_line.contains("```function_call")
            {
                Some("```")
            } else if first_line.contains("'''tool_call") {
                Some("'''")
            } else {
                None
            };
            self.pending.drain(..newline + 1);
            self.mode = ProtocolMode::IgnoreToolResult { closing };
            self.at_stream_start = false;
            return true;
        }

        if let Some((start, kind)) = self.find_tool_marker() {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }

            if !self.pending[kind.opening().len()..].contains('\n') && !final_flush {
                return false;
            }
            self.pending.drain(..kind.opening().len());
            self.mode = ProtocolMode::ToolBlock {
                kind,
                body: String::new(),
                oversized: false,
            };
            self.at_stream_start = false;
            return true;
        }

        if let Some(start) = self.pending.find(FOLLOW_UP_PREFIX) {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }

            let candidate = self.pending.clone();
            match parse_follow_up_candidates(&candidate) {
                Some(_) => {
                    self.pending.clear();
                    self.emit_follow_ups(&candidate, events);
                    self.at_stream_start = false;
                    return true;
                }
                None if !final_flush && candidate.len() <= MAX_FOLLOW_UP => return false,
                None => {
                    self.pending.clear();
                    self.at_stream_start = false;
                    return true;
                }
            }
        }

        if self.at_stream_start {
            let trimmed = self.pending.trim_start();
            if let Some(call) = parse_bare_json(trimmed, &mut self.next_call_id) {
                self.pending.clear();
                events.push(ProtocolEvent::ToolCall(call));
                self.at_stream_start = false;
                return true;
            }
        }

        if let Some(start) = self.find_partial_tool_marker() {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }
            return false;
        }

        if let Some(start) = partial_marker_suffix(&self.pending, FOLLOW_UP_PREFIX) {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }
            return false;
        }

        if self.pending.is_empty() {
            return false;
        }
        let text = std::mem::take(&mut self.pending);
        self.emit_text(&text, events);
        self.at_stream_start = false;
        false
    }

    fn find_tool_marker(&self) -> Option<(usize, BlockKind)> {
        [
            BlockKind::ToolCall,
            BlockKind::SingleQuoteToolCall,
            BlockKind::FunctionCall,
        ]
        .into_iter()
        .filter_map(|kind| self.pending.find(kind.opening()).map(|index| (index, kind)))
        .min_by_key(|(index, _)| *index)
    }

    fn find_partial_tool_marker(&self) -> Option<usize> {
        [
            BlockKind::ToolCall.opening(),
            BlockKind::SingleQuoteToolCall.opening(),
            BlockKind::FunctionCall.opening(),
        ]
        .into_iter()
        .filter_map(|marker| partial_marker_suffix(&self.pending, marker))
        .min()
    }

    fn drain_tool_result(&mut self, closing: Option<&'static str>, final_flush: bool) -> bool {
        match closing {
            Some(marker) => {
                if self.pending == marker {
                    self.pending.clear();
                    self.mode = ProtocolMode::Normal;
                    return true;
                }
                let needle = format!("\n{marker}");
                if let Some(index) = self.pending.find(&needle) {
                    self.pending.drain(..index + needle.len());
                    strip_protocol_separator(&mut self.pending);
                    self.mode = ProtocolMode::Normal;
                    return true;
                }
                if final_flush {
                    self.pending.clear();
                    self.mode = ProtocolMode::Normal;
                }
                false
            }
            None => {
                if let Some(index) = self.pending.find('\n') {
                    self.pending.drain(..index + 1);
                    self.mode = ProtocolMode::Normal;
                    return true;
                }
                if final_flush {
                    self.pending.clear();
                    self.mode = ProtocolMode::Normal;
                }
                false
            }
        }
    }

    fn drain_tool_block(&mut self, events: &mut Vec<ProtocolEvent>, final_flush: bool) -> bool {
        let closing = match &self.mode {
            ProtocolMode::ToolBlock { kind, .. } => kind.closing(),
            _ => unreachable!(),
        };

        let Some(end) = self.pending.find(closing) else {
            if let ProtocolMode::ToolBlock {
                body, oversized, ..
            } = &mut self.mode
            {
                if !*oversized {
                    body.push_str(&self.pending);
                    self.pending.clear();
                    if body.len() > MAX_TOOL_BLOCK {
                        *oversized = true;
                        body.clear();
                    }
                } else {
                    self.pending.clear();
                }
            }
            if final_flush {
                self.mode = ProtocolMode::Normal;
            }
            return false;
        };

        let fragment = self.pending[..end].to_owned();
        self.pending.drain(..end + closing.len());
        strip_protocol_separator(&mut self.pending);

        let (kind, body_text, oversized) = match &mut self.mode {
            ProtocolMode::ToolBlock {
                kind,
                body,
                oversized,
            } => {
                if !*oversized {
                    body.push_str(&fragment);
                }
                (*kind, std::mem::take(body), *oversized)
            }
            _ => unreachable!(),
        };

        self.mode = ProtocolMode::Normal;
        if !oversized {
            self.emit_tool_block(kind, &body_text, events);
        }
        true
    }

    fn emit_text(&self, text: &str, events: &mut Vec<ProtocolEvent>) {
        if !text.is_empty() {
            events.push(ProtocolEvent::Text(normalize_assistant_marker(text)));
        }
    }

    fn emit_tool_block(&mut self, _kind: BlockKind, body: &str, events: &mut Vec<ProtocolEvent>) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
            return;
        };
        let Some(name) = value.get("name").and_then(serde_json::Value::as_str) else {
            return;
        };
        let id = value
            .get("id")
            .or_else(|| value.get("call_id"))
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| allocate_call_id(&mut self.next_call_id));
        let arguments = value
            .get("arguments")
            .or_else(|| value.get("args"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        events.push(ProtocolEvent::ToolCall(ModelToolCall {
            id,
            name: name.to_owned(),
            arguments,
        }));
    }

    fn emit_follow_ups(&mut self, text: &str, events: &mut Vec<ProtocolEvent>) {
        if let Some(calls) = parse_follow_up_candidates(text) {
            for (label, query) in calls {
                events.push(ProtocolEvent::ToolCall(ModelToolCall {
                    id: self.allocate_id(),
                    name: "FollowUp".into(),
                    arguments: json!({ "label": label, "query": query }),
                }));
            }
        }
    }

    fn allocate_id(&mut self) -> String {
        allocate_call_id(&mut self.next_call_id)
    }
}

fn strip_protocol_separator(pending: &mut String) {
    if pending.starts_with("\r\n") {
        pending.drain(..2);
    } else if pending.starts_with('\n') || pending.starts_with('\r') {
        pending.drain(..1);
    }
}

fn partial_marker_suffix(input: &str, marker: &str) -> Option<usize> {
    let max = input.len().min(marker.len().saturating_sub(1));
    for len in (1..=max).rev() {
        let start = input.len() - len;
        if !input.is_char_boundary(start) {
            continue;
        }
        if marker.starts_with(&input[start..]) {
            return Some(start);
        }
    }
    None
}

fn normalize_assistant_marker(text: &str) -> String {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[Assistant]:") {
        rest.trim_start().to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(detector: &mut ProtocolDetector, chunks: &[&str]) -> Vec<ProtocolEvent> {
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(detector.feed(chunk));
        }
        events.extend(detector.finish());
        events
    }

    fn tool_call(event: &ProtocolEvent) -> (&str, &serde_json::Value) {
        match event {
            ProtocolEvent::ToolCall(call) => (&call.name, &call.arguments),
            other => panic!("expected tool call, got {other:?}"),
        }
    }

    #[test]
    fn detects_tool_call_after_assistant_text() {
        let mut detector = ProtocolDetector::default();
        let events = drain(
            &mut detector,
            &[
                "Je vais vérifier les fichiers.\n\n",
                "```tool_call\n",
                "{\"name\":\"glob\",\"arguments\":{\"pattern\":\"**/*\"}}\n",
                "```\n",
            ],
        );

        assert!(matches!(events[0], ProtocolEvent::Text(ref text) if text.contains("Je vais vérifier")));
        let (name, args) = tool_call(&events[1]);
        assert_eq!(name, "glob");
        assert_eq!(args["pattern"], "**/*");
    }

    #[test]
    fn continues_parsing_text_tool_text() {
        let mut detector = ProtocolDetector::default();
        let events = drain(
            &mut detector,
            &[
                "avant\n",
                "```tool_call\n{\"name\":\"list_directory\",\"arguments\":{}}\n```\n",
                "après",
            ],
        );

        assert!(matches!(events[0], ProtocolEvent::Text(ref text) if text.contains("avant")));
        assert_eq!(tool_call(&events[1]).0, "list_directory");
        assert!(matches!(events[2], ProtocolEvent::Text(ref text) if text.contains("après")));
    }

    #[test]
    fn preserves_tool_marker_split_across_chunks_after_text() {
        let mut detector = ProtocolDetector::default();
        let events = drain(
            &mut detector,
            &[
                "avant\n",
                "```to",
                "ol_call\n{\"name\":\"glob\",\"arguments\":{\"pattern\":\"*.rs\"}}\n```",
            ],
        );

        assert!(matches!(events[0], ProtocolEvent::Text(ref text) if text.contains("avant")));
        assert_eq!(tool_call(&events[1]).0, "glob");
    }

    #[test]
    fn detects_multiple_tool_calls_after_text() {
        let mut detector = ProtocolDetector::default();
        let events = drain(
            &mut detector,
            &[
                "Je vais inspecter le workspace.\n",
                "```tool_call\n{\"name\":\"list_directory\",\"arguments\":{}}\n```\n",
                "Puis chercher les fichiers.\n",
                "```tool_call\n{\"name\":\"glob\",\"arguments\":{\"pattern\":\"**/*\"}}\n```\n",
            ],
        );

        assert!(matches!(events[0], ProtocolEvent::Text(ref text) if text.contains("inspecter")));
        assert_eq!(tool_call(&events[1]).0, "list_directory");
        assert!(matches!(events[2], ProtocolEvent::Text(ref text) if text.contains("Puis chercher")));
        assert_eq!(tool_call(&events[3]).0, "glob");
    }

    #[test]
    fn does_not_drop_partial_tool_marker_at_stream_boundary() {
        let mut detector = ProtocolDetector::default();
        assert!(detector.feed("avant ```too").iter().any(|event| matches!(event, ProtocolEvent::Text(text) if text.contains("avant"))));
        assert!(detector.feed("l_call\n").is_empty());
        let events = detector.feed("{\"name\":\"glob\",\"arguments\":{}}\n```\n");
        assert_eq!(tool_call(&events[0]).0, "glob");
    }

    #[test]
    fn bare_json_tool_call_still_works_only_at_stream_start() {
        let mut detector = ProtocolDetector::default();
        let events = drain(
            &mut detector,
            &["{\"name\":\"glob\",\"arguments\":{\"pattern\":\"*.rs\"}}"],
        );
        assert_eq!(tool_call(&events[0]).0, "glob");
    }

    #[test]
    fn ordinary_text_containing_tool_word_is_preserved() {
        let mut detector = ProtocolDetector::default();
        let events = drain(&mut detector, &["Use the word tool_call in this explanation."]);
        assert!(matches!(events[0], ProtocolEvent::Text(ref text) if text.contains("tool_call")));
    }
}
