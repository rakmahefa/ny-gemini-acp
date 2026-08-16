//! Final presentation barrier for protocol syntax echoed by Gemini.
//!
//! Semantic parsing is deliberately outside this module:
//! - thinking is owned by `ThoughtStream`;
//! - tool calls are parsed from the raw response protocol stream;
//! - tool results are preserved by the runtime tool-history layer.
//!
//! This filter only prevents protocol envelopes from leaking into ACP-visible
//! assistant content. It is intentionally conservative and incremental so
//! protocol markers split across stream chunks cannot escape.

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";
const TOOL_CALL_FENCE: &str = "```tool_call";
const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCallFence {
    Backtick,
    SingleQuote,
}

impl ToolCallFence {
    const fn closing(self) -> &'static str {
        match self {
            Self::Backtick => "```",
            Self::SingleQuote => "'''",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropMode {
    None,
    ToolResultLine,
    ToolCallBlock(ToolCallFence),
}

/// Streaming protocol envelope filter used only at the ACP presentation edge.
#[derive(Debug)]
pub(crate) struct ProtocolFilter {
    pending: String,
    at_line_start: bool,
    drop_mode: DropMode,
    skipping_marker_spacing: bool,
    suppress_protocol_newline: bool,
}

impl Default for ProtocolFilter {
    fn default() -> Self {
        Self {
            pending: String::new(),
            at_line_start: true,
            drop_mode: DropMode::None,
            skipping_marker_spacing: false,
            suppress_protocol_newline: false,
        }
    }
}

impl ProtocolFilter {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, chunk: &str) -> String {
        self.process(chunk, false)
    }

    pub(crate) fn finish(&mut self) -> String {
        self.process("", true)
    }

    fn process(&mut self, chunk: &str, final_chunk: bool) -> String {
        let mut input = std::mem::take(&mut self.pending);
        input.push_str(chunk);

        let mut out = String::new();
        let mut i = 0;

        while i < input.len() {
            match self.drop_mode {
                DropMode::ToolResultLine => {
                    if input.as_bytes()[i] == b'\n' {
                        self.drop_mode = DropMode::None;
                        self.at_line_start = true;
                    }
                    i += 1;
                    continue;
                }
                DropMode::ToolCallBlock(fence) => {
                    let closing = fence.closing();

                    if self.at_line_start && input[i..].starts_with(closing) {
                        i += closing.len();
                        self.drop_mode = DropMode::None;
                        self.at_line_start = true;
                        self.suppress_protocol_newline = true;
                        continue;
                    }

                    if self.at_line_start && !final_chunk {
                        let keep = partial_suffix_len(&input[i..], closing);
                        if keep > 0 {
                            let end = input.len() - keep;
                            if end > i {
                                i = end;
                            }
                            self.pending.push_str(&input[i..]);
                            break;
                        }
                    }

                    let ch = input[i..].chars().next().expect("valid UTF-8 boundary");
                    i += ch.len_utf8();
                    self.at_line_start = ch == '\n';
                    continue;
                }
                DropMode::None => {}
            }

            if self.suppress_protocol_newline && input.as_bytes()[i] == b'\n' {
                i += 1;
                self.suppress_protocol_newline = false;
                self.at_line_start = true;
                continue;
            }
            self.suppress_protocol_newline = false;

            if self.skipping_marker_spacing {
                while i < input.len() && matches!(input.as_bytes()[i], b' ' | b'\t') {
                    i += 1;
                }
                self.skipping_marker_spacing = false;
                if i == input.len() {
                    return out;
                }
            }

            if self.at_line_start {
                let rest = &input[i..];

                if rest.starts_with(TOOL_RESULT_PREFIX) || rest.starts_with(TOOL_RESULT_ENVELOPE) {
                    let prefix_len = if rest.starts_with(TOOL_RESULT_PREFIX) {
                        TOOL_RESULT_PREFIX.len()
                    } else {
                        TOOL_RESULT_ENVELOPE.len()
                    };
                    self.drop_mode = DropMode::ToolResultLine;
                    i += prefix_len;
                    continue;
                }

                if rest.starts_with(TOOL_CALL_FENCE) {
                    self.drop_mode = DropMode::ToolCallBlock(ToolCallFence::Backtick);
                    i += TOOL_CALL_FENCE.len();
                    self.at_line_start = false;
                    continue;
                }

                if rest.starts_with(TOOL_CALL_SINGLE_QUOTE_FENCE) {
                    self.drop_mode = DropMode::ToolCallBlock(ToolCallFence::SingleQuote);
                    i += TOOL_CALL_SINGLE_QUOTE_FENCE.len();
                    self.at_line_start = false;
                    continue;
                }

                if rest.starts_with(ASSISTANT_MARKER) {
                    i += ASSISTANT_MARKER.len();
                    self.skipping_marker_spacing = true;
                    continue;
                }

                if rest.starts_with(USER_MARKER) {
                    i += USER_MARKER.len();
                    self.skipping_marker_spacing = true;
                    continue;
                }

                let prefixes = [
                    TOOL_RESULT_PREFIX,
                    TOOL_RESULT_ENVELOPE,
                    TOOL_CALL_FENCE,
                    TOOL_CALL_SINGLE_QUOTE_FENCE,
                    ASSISTANT_MARKER,
                    USER_MARKER,
                ];
                if !final_chunk && prefixes.iter().any(|prefix| prefix.starts_with(rest)) {
                    self.pending.push_str(rest);
                    break;
                }
            }

            let ch = input[i..].chars().next().expect("valid UTF-8 boundary");
            out.push(ch);
            self.at_line_start = ch == '\n';
            i += ch.len_utf8();
        }

        // A pending protocol prefix is never considered user-visible text at EOF.
        if final_chunk {
            self.pending.clear();
            self.skipping_marker_spacing = false;
        }

        out
    }
}

fn partial_suffix_len(text: &str, needle: &str) -> usize {
    let max = text.len().min(needle.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&needle[..len]) {
            return len;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize(text: &str) -> String {
        let mut filter = ProtocolFilter::new();
        let mut output = filter.push(text);
        output.push_str(&filter.finish());
        output
    }

    #[test]
    fn filters_protocol_envelopes() {
        assert_eq!(sanitize("[Assistant]: Réponse"), "Réponse");
        assert_eq!(sanitize("[User]: question"), "question");
        assert_eq!(sanitize("[Tool result for shell_exec]: done"), "");
        assert_eq!(sanitize("[Tool result]: payload"), "");
    }

    #[test]
    fn filters_tool_call_blocks_without_parsing_the_body() {
        let input = "```tool_call\n{\"value\":\"```\"}\n```\n[Assistant]: Réponse";
        assert_eq!(sanitize(input), "Réponse");
    }

    #[test]
    fn filters_single_quote_tool_call_blocks_without_parsing_the_body() {
        let input = "'''tool_call\n{\"value\":\"'''\"}\n'''\n[Assistant]: Réponse";
        assert_eq!(sanitize(input), "Réponse");
    }

    #[test]
    fn preserves_thinking_like_text() {
        let input = "<thinking>ceci est du texte visible</thinking>";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn preserves_normal_markdown_fences() {
        let input = "Voici un exemple :\n```rust\nfn main() {}\n```";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn preserves_normal_python_triple_quotes() {
        let input = "Voici du Python :\n'''docstring'''\nprint('ok')";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn buffers_protocol_opening_marker_across_chunks() {
        let mut filter = ProtocolFilter::new();
        assert_eq!(filter.push("[Assis"), "");
        assert_eq!(filter.push("tant]:"), "");
        assert_eq!(filter.push(" suite"), "suite");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn buffers_tool_call_opening_across_chunks() {
        let mut filter = ProtocolFilter::new();
        assert_eq!(filter.push("```tool_"), "");
        assert_eq!(filter.push("call\n{}\n```\n"), "");
        assert_eq!(filter.push("Réponse"), "Réponse");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn buffers_closing_fence_at_every_boundary() {
        for closing in ["```", "'''"] {
            for split in 1..closing.len() {
                let mut filter = ProtocolFilter::new();
                let opening = if closing == "```" {
                    "```tool_call\n{}\n"
                } else {
                    "'''tool_call\n{}\n"
                };
                assert_eq!(filter.push(opening), "");
                assert_eq!(filter.push(&closing[..split]), "");
                assert_eq!(filter.push(&closing[split..]), "");
                assert_eq!(filter.push("\n[Assistant]: suite"), "suite");
                assert_eq!(filter.finish(), "");
            }
        }
    }

    #[test]
    fn keeps_unclosed_protocol_block_hidden_at_stream_end() {
        let mut filter = ProtocolFilter::new();
        assert_eq!(filter.push("```tool_call\n{\"secret\":true}\n"), "");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn removes_tool_result_before_assistant_in_same_stream() {
        let mut filter = ProtocolFilter::new();
        assert_eq!(
            filter.push("[Tool result for shell_exec]: done\n[Assistant]: Suite"),
            "Suite"
        );
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_split_tool_result_envelope_without_reinterpreting_payload() {
        let mut filter = ProtocolFilter::new();
        assert_eq!(filter.push("[Tool res"), "");
        assert_eq!(
            filter.push(
                "ult]: {\"tool\":\"file_read\",\"content\":\"x\\n'''\\n```\"}\n[Assistant]: Réponse"
            ),
            "Réponse"
        );
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn preserves_text_around_filtered_lines() {
        assert_eq!(
            sanitize("Avant\n[Tool result for shell_exec]: done\nAprès"),
            "Avant\nAprès"
        );
    }
}
