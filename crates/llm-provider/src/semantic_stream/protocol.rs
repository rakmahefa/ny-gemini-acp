use serde_json::json;

use super::parsers::{allocate_call_id, parse_bare_json, parse_follow_up_candidates};
use super::types::{
    BlockKind, ModelToolCall, ProtocolEvent, ProtocolMode, MAX_FOLLOW_UP, MAX_PENDING,
    MAX_TOOL_BLOCK, TOOL_RESULT_ENVELOPE, TOOL_RESULT_PREFIX, FOLLOW_UP_PREFIX,
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

        for kind in [
            BlockKind::ToolCall,
            BlockKind::SingleQuoteToolCall,
            BlockKind::FunctionCall,
        ] {
            if self.pending.starts_with(kind.opening()) {
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
            if is_prefix(&self.pending, kind.opening()) {
                return false;
            }
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

        if self.pending.is_empty() {
            return false;
        }
        let text = std::mem::take(&mut self.pending);
        self.emit_text(&text, events);
        self.at_stream_start = false;
        !final_flush
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

fn is_prefix(value: &str, marker: &str) -> bool {
    value.len() < marker.len() && marker.starts_with(value)
}

fn normalize_assistant_marker(text: &str) -> String {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[Assistant]:") {
        rest.trim_start().to_owned()
    } else {
        text.to_owned()
    }
}
