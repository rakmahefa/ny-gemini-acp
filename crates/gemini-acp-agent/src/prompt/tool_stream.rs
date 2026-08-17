//! Incremental Gemini tool-protocol detection.
//!
//! The detector deliberately operates on the raw response stream, but only
//! interprets protocol envelopes that start at line boundaries. Tool-result
//! lines are opaque and are never reparsed, even when their payload contains
//! strings that look like tool-call envelopes.

use gemini_acp_runtime::tools::parse::{parse_tool_calls, ParsedToolCall};

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";
const TOOL_CALL_FENCE: &str = "```tool_call";
const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";
const FUNCTION_CALL_FENCE: &str = "```function_call";
const MAX_PROTOCOL_BLOCK: usize = 256 * 1024;
const MAX_FOLLOW_UP_TAG: usize = 64 * 1024;
const MAX_BARE_JSON: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    ToolCall,
    ToolCallSingleQuote,
    FunctionCall,
}

#[derive(Debug)]
enum Mode {
    Normal,
    IgnoreLine,
    Block {
        kind: BlockKind,
        body: String,
        line_start: bool,
        close_probe: String,
        oversized: bool,
    },
}

/// Incremental semantic detector for executable tool protocol.
#[derive(Debug)]
pub(crate) struct ToolStreamDetector {
    mode: Mode,
    line_start: bool,
    line_probe: String,
    follow_up: Option<(String, Option<char>)>,
    bare_json: Option<String>,
    plain_prefix_only: bool,
}

impl Default for ToolStreamDetector {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            line_start: true,
            line_probe: String::new(),
            follow_up: None,
            bare_json: None,
            plain_prefix_only: true,
        }
    }
}

impl ToolStreamDetector {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn feed(&mut self, chunk: &str) -> Vec<ParsedToolCall> {
        self.process(chunk, false)
    }

    pub(crate) fn finish(&mut self) -> Vec<ParsedToolCall> {
        let mut calls = self.process("", true);
        self.follow_up = None;
        if let Some(candidate) = self.bare_json.take() {
            calls.extend(parse_tool_calls(&candidate).1);
        }
        calls
    }

    fn process(&mut self, chunk: &str, final_chunk: bool) -> Vec<ParsedToolCall> {
        let mut calls = Vec::new();
        for ch in chunk.chars() {
            self.process_char(ch, &mut calls);
        }
        if final_chunk {
            calls.extend(self.finish_block_if_complete());
        }
        calls
    }

    fn process_char(&mut self, ch: char, calls: &mut Vec<ParsedToolCall>) {
        match &mut self.mode {
            Mode::IgnoreLine => {
                if ch == '\n' {
                    self.mode = Mode::Normal;
                    self.line_start = true;
                    self.line_probe.clear();
                }
                return;
            }
            Mode::Block {
                kind,
                body,
                line_start,
                close_probe,
                oversized,
            } => {
                let closing = match kind {
                    BlockKind::ToolCall => "```",
                    BlockKind::ToolCallSingleQuote => "'''",
                    BlockKind::FunctionCall => "```",
                };

                if *line_start || !close_probe.is_empty() {
                    if ch == closing.as_bytes()[close_probe.len()] as char {
                        close_probe.push(ch);
                        if close_probe == closing {
                            let body = std::mem::take(body);
                            let kind = *kind;
                            let was_oversized = *oversized;
                            self.mode = Mode::Normal;
                            self.line_start = true;
                            close_probe.clear();
                            if !was_oversized {
                                calls.extend(parse_block(kind, &body));
                            }
                            return;
                        }
                        return;
                    }
                    if !close_probe.is_empty() {
                        if !*oversized {
                            body.push_str(close_probe);
                        }
                        close_probe.clear();
                    }
                }

                if ch == '\n' {
                    *line_start = true;
                } else {
                    *line_start = false;
                }

                if !*oversized {
                    body.push(ch);
                    if body.len() > MAX_PROTOCOL_BLOCK {
                        *oversized = true;
                        body.clear();
                    }
                }
                return;
            }
            Mode::Normal => {}
        }

        if self.line_start {
            self.line_probe.push(ch);
            if let Some(kind) = complete_opening(&self.line_probe) {
                self.line_probe.clear();
                self.mode = match kind {
                    Opening::IgnoreLine => Mode::IgnoreLine,
                    Opening::Block(kind) => Mode::Block {
                        kind,
                        body: String::new(),
                        line_start: false,
                        close_probe: String::new(),
                        oversized: false,
                    },
                    Opening::Marker => Mode::Normal,
                };
                if matches!(self.mode, Mode::Block { .. } | Mode::IgnoreLine) {
                    return;
                }
            }

            if !could_continue_opening(&self.line_probe) {
                let probe = std::mem::take(&mut self.line_probe);
                self.line_start = false;
                for probe_ch in probe.chars() {
                    self.process_normal_char(probe_ch, calls);
                }
                return;
            }

            if ch == '\n' {
                let probe = std::mem::take(&mut self.line_probe);
                self.line_start = true;
                for probe_ch in probe.chars() {
                    if probe_ch != '\n' {
                        self.process_normal_char(probe_ch, calls);
                    }
                }
            }
            return;
        }

        self.process_normal_char(ch, calls);
        if ch == '\n' {
            self.line_start = true;
            self.line_probe.clear();
        }
    }

    fn process_normal_char(&mut self, ch: char, calls: &mut Vec<ParsedToolCall>) {
        if let Some((tag, quote)) = &mut self.follow_up {
            tag.push(ch);
            if tag.len() > MAX_FOLLOW_UP_TAG {
                self.follow_up = None;
                return;
            }
            match quote {
                Some(current) if ch == *current => *quote = None,
                None if ch == '\'' || ch == '"' => *quote = Some(ch),
                None if ch == '>' => {
                    let tag = std::mem::take(tag);
                    self.follow_up = None;
                    calls.extend(parse_tool_calls(&tag).1);
                }
                _ => {}
            }
            return;
        }

        if ch == '<' {
            self.follow_up = Some(("<".to_owned(), None));
            return;
        }

        if let Some(candidate) = &mut self.bare_json {
            candidate.push(ch);
            if candidate.len() > MAX_BARE_JSON {
                self.bare_json = None;
                return;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) {
                let candidate = std::mem::take(candidate);
                if value
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
                    && (value.get("arguments").is_some() || value.get("args").is_some())
                {
                    calls.extend(parse_tool_calls(&candidate).1);
                }
                self.plain_prefix_only = false;
            }
            return;
        }

        if self.plain_prefix_only {
            if ch.is_whitespace() {
                return;
            }
            if ch == '{' {
                self.bare_json = Some("{".to_owned());
                return;
            }
            self.plain_prefix_only = false;
        }
    }

    fn finish_block_if_complete(&mut self) -> Vec<ParsedToolCall> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opening {
    IgnoreLine,
    Block(BlockKind),
    Marker,
}

fn complete_opening(probe: &str) -> Option<Opening> {
    match probe {
        TOOL_RESULT_ENVELOPE | TOOL_RESULT_PREFIX => Some(Opening::IgnoreLine),
        TOOL_CALL_FENCE => Some(Opening::Block(BlockKind::ToolCall)),
        TOOL_CALL_SINGLE_QUOTE_FENCE => Some(Opening::Block(BlockKind::ToolCallSingleQuote)),
        FUNCTION_CALL_FENCE => Some(Opening::Block(BlockKind::FunctionCall)),
        ASSISTANT_MARKER | USER_MARKER => Some(Opening::Marker),
        _ => None,
    }
}

fn could_continue_opening(probe: &str) -> bool {
    [
        TOOL_RESULT_PREFIX,
        TOOL_RESULT_ENVELOPE,
        TOOL_CALL_FENCE,
        TOOL_CALL_SINGLE_QUOTE_FENCE,
        FUNCTION_CALL_FENCE,
        ASSISTANT_MARKER,
        USER_MARKER,
    ]
    .iter()
    .any(|prefix| prefix.starts_with(probe))
}

fn parse_block(kind: BlockKind, body: &str) -> Vec<ParsedToolCall> {
    let normalized = match kind {
        BlockKind::ToolCall | BlockKind::ToolCallSingleQuote => {
            format!("```tool_call\n{body}\n```")
        }
        BlockKind::FunctionCall => format!("```function_call\n{body}\n```"),
    };
    parse_tool_calls(&normalized).1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: &[&str]) -> Vec<ParsedToolCall> {
        let mut detector = ToolStreamDetector::new();
        let mut calls = Vec::new();
        for chunk in chunks {
            calls.extend(detector.feed(chunk));
        }
        calls.extend(detector.finish());
        calls
    }

    #[test]
    fn detects_tool_call_incrementally() {
        let calls = collect(&[
            "```tool_",
            "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{\"command\":\"cargo test\"}}\n",
            "```\n",
        ]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
    }

    #[test]
    fn detects_function_call_incrementally() {
        let calls = collect(&[
            "```function_call\n{\"name\":\"shell_exec\",\"args\":{}}\n```",
        ]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell_exec");
    }

    #[test]
    fn ignores_tool_result_payload_even_when_it_contains_tool_protocol() {
        let calls = collect(&[
            "[Tool result for file_read]: ```tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n```\n",
            "[Assistant]: suite",
        ]);
        assert!(calls.is_empty());
    }

    #[test]
    fn follows_split_tool_result_marker() {
        let calls = collect(&[
            "[Tool res",
            "ult]: {\"content\":\"```tool_call\\n{\\\"name\\\":\\\"shell_exec\\\"}\\n```\"}\n",
        ]);
        assert!(calls.is_empty());
    }

    #[test]
    fn detects_follow_up_incrementally() {
        let calls = collect(&[
            "Réponse visible\n<FollowUp label=\"Tests\" ",
            "query=\"cargo test\" />",
        ]);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].is_action());
    }

    #[test]
    fn detects_bare_json_tool_call_at_stream_prefix() {
        let calls = collect(&[
            "{\"name\":\"shell_exec\",\"arguments\":{\"command\":\"pwd\"}}",
        ]);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn normal_json_without_tool_shape_is_not_a_call() {
        let calls = collect(&["{\"name\":\"project\",\"value\":42}"]);
        assert!(calls.is_empty());
    }

    #[test]
    fn single_quote_tool_call_is_supported() {
        let calls = collect(&["'''tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n'''\n"]);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn unclosed_block_never_becomes_a_tool_call() {
        let calls = collect(&["```tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}"]);
        assert!(calls.is_empty());
    }
}
