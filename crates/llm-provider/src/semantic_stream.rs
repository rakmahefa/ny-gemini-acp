use agent_runtime::ModelEvent;
use serde_json::{json, Value};

const REASONING_OPEN_MARKERS: [&str; 4] = ["<thinking>", "<think>", "[Thinking]:", "[thinking]:"];
const REASONING_CLOSE_MARKERS: [&str; 2] = ["</thinking>", "</think>"];
const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
const TOOL_CALL_FENCE: &str = "```tool_call";
const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";
const FUNCTION_CALL_FENCE: &str = "```function_call";
const FOLLOW_UP_PREFIX: &str = "<FollowUp";
const MAX_PENDING: usize = 256 * 1024;
const MAX_FOLLOW_UP: usize = 64 * 1024;
const MAX_TOOL_BLOCK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReasoningPhase { Detecting, Response, Reasoning, Completed }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind { ToolCall, FunctionCall, SingleQuoteToolCall }
impl BlockKind {
    fn opening(self) -> &'static str { match self { Self::ToolCall => TOOL_CALL_FENCE, Self::FunctionCall => FUNCTION_CALL_FENCE, Self::SingleQuoteToolCall => TOOL_CALL_SINGLE_QUOTE_FENCE } }
    fn closing(self) -> &'static str { match self { Self::SingleQuoteToolCall => "'''", Self::ToolCall | Self::FunctionCall => "```" } }
}

#[derive(Debug)]
enum ProtocolMode { Normal, IgnoreToolResult { closing: Option<&'static str> }, ToolBlock { kind: BlockKind, body: String, oversized: bool } }
#[derive(Debug)]
struct ProtocolDetector { mode: ProtocolMode, pending: String, at_stream_start: bool, next_call_id: usize }
impl Default for ProtocolDetector { fn default() -> Self { Self { mode: ProtocolMode::Normal, pending: String::new(), at_stream_start: true, next_call_id: 0 } } }

impl ProtocolDetector {
    fn feed(&mut self, chunk: &str) -> Vec<ProtocolEvent> {
        if chunk.is_empty() { return Vec::new(); }
        self.pending.push_str(chunk);
        if self.pending.len() > MAX_PENDING { self.pending.clear(); self.mode = ProtocolMode::Normal; return Vec::new(); }
        self.drain(false)
    }
    fn finish(&mut self) -> Vec<ProtocolEvent> { let events = self.drain(true); self.mode = ProtocolMode::Normal; self.pending.clear(); events }
    fn drain(&mut self, final_flush: bool) -> Vec<ProtocolEvent> {
        let mut events = Vec::new();
        loop {
            let progressed = match self.mode {
                ProtocolMode::Normal => self.drain_normal(&mut events, final_flush),
                ProtocolMode::IgnoreToolResult { closing } => self.drain_tool_result(closing, final_flush),
                ProtocolMode::ToolBlock { .. } => self.drain_tool_block(&mut events, final_flush),
            };
            if !progressed { break; }
        }
        events
    }
    fn drain_normal(&mut self, events: &mut Vec<ProtocolEvent>, final_flush: bool) -> bool {
        if let Some(start) = self.pending.find(TOOL_RESULT_PREFIX).or_else(|| self.pending.find(TOOL_RESULT_ENVELOPE)) {
            if start > 0 {
                let text = self.pending[..start].to_owned();
                self.pending.drain(..start);
                self.emit_text(&text, events);
                self.at_stream_start = false;
                return true;
            }
            let Some(newline) = self.pending.find('\n') else { if final_flush { self.pending.clear(); } return false; };
            let first_line = self.pending[..newline].trim_end_matches(['\r','\n']);
            let closing = if first_line.contains(TOOL_CALL_FENCE) || first_line.contains(FUNCTION_CALL_FENCE) { Some("```") } else if first_line.contains(TOOL_CALL_SINGLE_QUOTE_FENCE) { Some("'''") } else { None };
            self.pending.drain(..newline + 1);
            self.mode = ProtocolMode::IgnoreToolResult { closing };
            self.at_stream_start = false;
            return true;
        }
        for kind in [BlockKind::ToolCall, BlockKind::SingleQuoteToolCall, BlockKind::FunctionCall] {
            if self.pending.starts_with(kind.opening()) {
                if !self.pending[kind.opening().len()..].contains('\n') && !final_flush { return false; }
                self.pending.drain(..kind.opening().len());
                self.mode = ProtocolMode::ToolBlock { kind, body: String::new(), oversized: false };
                self.at_stream_start = false;
                return true;
            }
            if is_prefix(&self.pending, kind.opening()) { return false; }
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
                Some(_) => { self.pending.clear(); self.emit_follow_ups(&candidate, events); self.at_stream_start = false; return true; }
                None if !final_flush && candidate.len() <= MAX_FOLLOW_UP => return false,
                None => { self.pending.clear(); self.at_stream_start = false; return true; }
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
        if self.pending.is_empty() { return false; }
        let text = std::mem::take(&mut self.pending);
        self.emit_text(&text, events);
        self.at_stream_start = false;
        !final_flush
    }
    fn drain_tool_result(&mut self, closing: Option<&'static str>, final_flush: bool) -> bool {
        match closing {
            Some(marker) => {
                if self.pending == marker { self.pending.clear(); self.mode = ProtocolMode::Normal; return true; }
                let needle = format!("\n{marker}");
                if let Some(index) = self.pending.find(&needle) { self.pending.drain(..index + needle.len()); self.mode = ProtocolMode::Normal; return true; }
                if final_flush { self.pending.clear(); self.mode = ProtocolMode::Normal; }
                false
            }
            None => {
                if let Some(index) = self.pending.find('\n') { self.pending.drain(..index + 1); self.mode = ProtocolMode::Normal; return true; }
                if final_flush { self.pending.clear(); self.mode = ProtocolMode::Normal; }
                false
            }
        }
    }
    fn drain_tool_block(&mut self, events: &mut Vec<ProtocolEvent>, final_flush: bool) -> bool {
        let closing = match &self.mode { ProtocolMode::ToolBlock { kind, .. } => kind.closing(), _ => unreachable!() };
        let Some(end) = self.pending.find(closing) else {
            if let ProtocolMode::ToolBlock { body, oversized, .. } = &mut self.mode {
                if !*oversized { body.push_str(&self.pending); self.pending.clear(); if body.len() > MAX_TOOL_BLOCK { *oversized = true; body.clear(); } } else { self.pending.clear(); }
            }
            if final_flush { self.mode = ProtocolMode::Normal; }
            return false;
        };
        let fragment = self.pending[..end].to_owned();
        self.pending.drain(..end + closing.len());
        let (kind, body_text, oversized) = match &mut self.mode {
            ProtocolMode::ToolBlock { kind, body, oversized } => { if !*oversized { body.push_str(&fragment); } (*kind, std::mem::take(body), *oversized) }
            _ => unreachable!(),
        };
        self.mode = ProtocolMode::Normal;
        if !oversized { self.emit_tool_block(kind, &body_text, events); }
        true
    }
    fn emit_text(&self, text: &str, events: &mut Vec<ProtocolEvent>) { if !text.is_empty() { events.push(ProtocolEvent::Text(normalize_assistant_marker(text))); } }
    fn emit_tool_block(&mut self, _kind: BlockKind, body: &str, events: &mut Vec<ProtocolEvent>) {
        let Ok(value) = serde_json::from_str::<Value>(body.trim()) else { return; };
        let Some(name) = value.get("name").and_then(Value::as_str) else { return; };
        let id = value.get("id").or_else(|| value.get("call_id")).and_then(Value::as_str).filter(|id| !id.trim().is_empty()).map(ToOwned::to_owned).unwrap_or_else(|| self.allocate_id());
        let arguments = value.get("arguments").or_else(|| value.get("args")).cloned().unwrap_or_else(|| json!({}));
        events.push(ProtocolEvent::ToolCall(ModelToolCall { id, name: name.to_owned(), arguments }));
    }
    fn emit_follow_ups(&mut self, text: &str, events: &mut Vec<ProtocolEvent>) { if let Some(calls) = parse_follow_up_candidates(text) { for (label, query) in calls { events.push(ProtocolEvent::ToolCall(ModelToolCall { id: self.allocate_id(), name: "FollowUp".into(), arguments: json!({"label":label,"query":query}) })); } } }
    fn allocate_id(&mut self) -> String { let id = format!("gemini_call_{}", self.next_call_id); self.next_call_id = self.next_call_id.saturating_add(1); id }
}

#[derive(Debug)]
enum ProtocolEvent { Text(String), ToolCall(ModelToolCall) }
#[derive(Debug)]
struct ModelToolCall { id: String, name: String, arguments: Value }

#[derive(Debug)]
pub struct GeminiSemanticStream { reasoning_phase: ReasoningPhase, reasoning_pending: String, protocol: ProtocolDetector, completed: bool }
impl GeminiSemanticStream {
    pub fn new(supports_reasoning: bool) -> Self { Self { reasoning_phase: if supports_reasoning { ReasoningPhase::Detecting } else { ReasoningPhase::Response }, reasoning_pending: String::new(), protocol: ProtocolDetector::default(), completed: false } }
    pub fn feed(&mut self, delta: &str) -> Vec<ModelEvent> { if delta.is_empty() || self.completed { return Vec::new(); } self.protocol.feed(delta).into_iter().flat_map(|event| self.project_protocol_event(event)).collect() }
    pub fn finish(&mut self) -> Vec<ModelEvent> {
        if self.completed { return Vec::new(); }
        let mut output = Vec::new();
        for event in self.protocol.finish() { output.extend(self.project_protocol_event(event)); }
        let pending = std::mem::take(&mut self.reasoning_pending);
        if !pending.is_empty() { match self.reasoning_phase { ReasoningPhase::Reasoning => output.push(ModelEvent::ReasoningDelta(pending)), ReasoningPhase::Detecting | ReasoningPhase::Response => output.push(ModelEvent::TextDelta(pending)), ReasoningPhase::Completed => {} } }
        self.reasoning_phase = ReasoningPhase::Completed;
        self.completed = true;
        output
    }
    fn project_protocol_event(&mut self, event: ProtocolEvent) -> Vec<ModelEvent> { match event { ProtocolEvent::Text(text) => self.feed_reasoning(text), ProtocolEvent::ToolCall(call) => vec![ModelEvent::ToolCall { id: call.id, name: call.name, arguments: call.arguments }] } }
    fn feed_reasoning(&mut self, delta: String) -> Vec<ModelEvent> { if delta.is_empty() { return Vec::new(); } match self.reasoning_phase { ReasoningPhase::Response => vec![ModelEvent::TextDelta(delta)], ReasoningPhase::Detecting => self.feed_reasoning_detecting(&delta), ReasoningPhase::Reasoning => self.feed_reasoning_body(&delta), ReasoningPhase::Completed => Vec::new() } }
    fn feed_reasoning_detecting(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.reasoning_pending.push_str(delta);
        if let Some(marker_len) = matching_marker_len(&self.reasoning_pending, &REASONING_OPEN_MARKERS) {
            self.reasoning_pending.drain(..marker_len);
            self.reasoning_phase = ReasoningPhase::Reasoning;
            let pending = std::mem::take(&mut self.reasoning_pending);
            return self.feed_reasoning(pending);
        }
        let keep = partial_suffix_for_markers(&self.reasoning_pending, &REASONING_OPEN_MARKERS);
        if self.reasoning_pending.len() <= keep { return Vec::new(); }
        let split_at = self.reasoning_pending.len() - keep;
        let response = self.reasoning_pending[..split_at].to_owned();
        self.reasoning_pending.drain(..split_at);
        self.reasoning_phase = ReasoningPhase::Response;
        if response.is_empty() { Vec::new() } else { vec![ModelEvent::TextDelta(response)] }
    }
    fn feed_reasoning_body(&mut self, delta: &str) -> Vec<ModelEvent> {
        self.reasoning_pending.push_str(delta);
        if let Some((idx, marker_len)) = find_marker(&self.reasoning_pending, &REASONING_CLOSE_MARKERS) {
            let reasoning = self.reasoning_pending[..idx].to_owned();
            let response = self.reasoning_pending[idx + marker_len..].to_owned();
            self.reasoning_pending.clear();
            self.reasoning_phase = ReasoningPhase::Response;
            let mut events = Vec::new();
            if !reasoning.is_empty() { events.push(ModelEvent::ReasoningDelta(reasoning)); }
            if !response.is_empty() { events.push(ModelEvent::TextDelta(response)); }
            return events;
        }
        let keep = partial_suffix_for_markers(&self.reasoning_pending, &REASONING_CLOSE_MARKERS);
        if self.reasoning_pending.len() <= keep { return Vec::new(); }
        let split_at = self.reasoning_pending.len() - keep;
        let reasoning = self.reasoning_pending[..split_at].to_owned();
        self.reasoning_pending.drain(..split_at);
        if reasoning.is_empty() { Vec::new() } else { vec![ModelEvent::ReasoningDelta(reasoning)] }
    }
}

fn is_prefix(value: &str, marker: &str) -> bool { value.len() < marker.len() && marker.starts_with(value) }
fn normalize_assistant_marker(text: &str) -> String { let trimmed = text.trim_start(); if let Some(rest) = trimmed.strip_prefix("[Assistant]:") { rest.trim_start().to_owned() } else { text.to_owned() } }
fn matching_marker_len(buffer: &str, markers: &[&str]) -> Option<usize> { markers.iter().find(|marker| buffer.starts_with(**marker)).map(|marker| marker.len()) }
fn partial_suffix_for_markers(text: &str, markers: &[&str]) -> usize { markers.iter().map(|marker| partial_suffix_len(text, marker)).max().unwrap_or(0) }
fn partial_suffix_len(text: &str, marker: &str) -> usize { let max = text.len().min(marker.len().saturating_sub(1)); for len in (1..=max).rev() { if text.ends_with(&marker[..len]) { return len; } } 0 }
fn find_marker(buffer: &str, markers: &[&str]) -> Option<(usize, usize)> { markers.iter().filter_map(|marker| buffer.find(marker).map(|idx| (idx, marker.len()))).min_by_key(|(idx, _)| *idx) }
fn parse_bare_json(text: &str, next_id: &mut usize) -> Option<ModelToolCall> { let value = serde_json::from_str::<Value>(text.trim()).ok()?; let name = value.get("name").and_then(Value::as_str)?.trim(); if name.is_empty() || (value.get("arguments").is_none() && value.get("args").is_none()) { return None; } let arguments = value.get("arguments").or_else(|| value.get("args")).cloned().unwrap_or_else(|| json!({})); let id = value.get("id").or_else(|| value.get("call_id")).and_then(Value::as_str).filter(|id| !id.trim().is_empty()).map(ToOwned::to_owned).unwrap_or_else(|| { let id = format!("gemini_call_{}", *next_id); *next_id = next_id.saturating_add(1); id }); Some(ModelToolCall { id, name: name.to_owned(), arguments }) }
fn parse_follow_up_candidates(text: &str) -> Option<Vec<(String, String)>> { let mut cursor = 0; let mut found = false; let mut calls = Vec::new(); while let Some(relative_start) = text[cursor..].find(FOLLOW_UP_PREFIX) { found = true; let start = cursor + relative_start; let after = start + FOLLOW_UP_PREFIX.len(); let end = find_tag_end(&text[after..])?; let absolute_end = after + end; let tag = &text[start..=absolute_end]; calls.push(parse_follow_up_tag(tag)?); cursor = absolute_end + 1; } if found { Some(calls) } else { None } }
fn parse_follow_up_tag(tag: &str) -> Option<(String, String)> { let inner = tag.strip_prefix(FOLLOW_UP_PREFIX)?.strip_suffix('>')?.trim(); let inner = inner.strip_suffix('/').unwrap_or(inner).trim(); let attrs = parse_attributes(inner); let label = attrs.get("label")?.trim(); let query = attrs.get("query")?.trim(); if label.is_empty() || query.is_empty() { return None; } Some((decode_xml(label), decode_xml(query))) }
fn parse_attributes(input: &str) -> std::collections::BTreeMap<String, String> { let mut attrs = std::collections::BTreeMap::new(); let bytes = input.as_bytes(); let mut index = 0; while index < bytes.len() { while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; } if index >= bytes.len() || bytes[index] == b'/' { break; } let key_start = index; while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' { index += 1; } if key_start == index { index += 1; continue; } let key = &input[key_start..index]; while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; } if index >= bytes.len() || bytes[index] != b'=' { break; } index += 1; while index < bytes.len() && bytes[index].is_ascii_whitespace() { index += 1; } if index >= bytes.len() { break; } let value = if bytes[index] == b'\'' || bytes[index] == b'"' { let quote = bytes[index]; index += 1; let value_start = index; while index < bytes.len() && bytes[index] != quote { index += 1; } let value = input[value_start..index].to_owned(); if index < bytes.len() { index += 1; } value } else { let value_start = index; while index < bytes.len() && !bytes[index].is_ascii_whitespace() { index += 1; } input[value_start..index].to_owned() }; attrs.insert(key.to_ascii_lowercase(), value); } attrs }
fn decode_xml(input: &str) -> String { input.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&") }
fn find_tag_end(input: &str) -> Option<usize> { let mut quote = None; for (index, byte) in input.as_bytes().iter().copied().enumerate() { match quote { Some(current) if byte == current => quote = None, Some(_) => {}, None if byte == b'\'' || byte == b'"' => quote = Some(byte), None if byte == b'>' => return Some(index), None => {} } } None }

#[cfg(test)]
mod tests {
    use super::*;
    fn collect(chunks: &[&str]) -> Vec<ModelEvent> { let mut stream = GeminiSemanticStream::new(true); let mut out = Vec::new(); for chunk in chunks { out.extend(stream.feed(chunk)); } out.extend(stream.finish()); out }
    #[test] fn reasoning_envelope_becomes_semantic_events() { assert_eq!(collect(&["<thinking>raisonnement", " utile</thinking>", "réponse"]), vec![ModelEvent::ReasoningDelta("raisonnement".into()), ModelEvent::ReasoningDelta(" utile".into()), ModelEvent::TextDelta("réponse".into())]); }
    #[test] fn reasoning_marker_split_across_chunks_is_atomic() { assert_eq!(collect(&["<thi", "nking>pensée</thinking>réponse"]), vec![ModelEvent::ReasoningDelta("pensée".into()), ModelEvent::TextDelta("réponse".into())]); }
    #[test] fn detects_tool_call_incrementally() { assert_eq!(collect(&["```tool_", "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n```\n"]), vec![ModelEvent::ToolCall { id: "c1".into(), name: "shell_exec".into(), arguments: json!({}) }]); }
    #[test] fn detects_follow_up_incrementally() { assert_eq!(collect(&["<FollowUp label=\"Run\" ", "query=\"cargo test\" />"]), vec![ModelEvent::ToolCall { id: "gemini_call_0".into(), name: "FollowUp".into(), arguments: json!({"label":"Run","query":"cargo test"}) }]); }
    #[test] fn ignores_tool_result_payload() { assert_eq!(collect(&["[Tool result for shell_exec]: ```tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n```\nanswer\n"]), vec![ModelEvent::TextDelta("answer\n".into())]); }
    #[test] fn non_reasoning_models_pass_through_reasoning_markers() { let mut stream = GeminiSemanticStream::new(false); let mut events = stream.feed("<thinking>hidden</thinking>answer"); events.extend(stream.finish()); assert_eq!(events, vec![ModelEvent::TextDelta("<thinking>hidden</thinking>answer".into())]); }
}
