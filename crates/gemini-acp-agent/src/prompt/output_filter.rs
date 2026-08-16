//! Filtrage incrémental de la sortie Gemini avant émission ACP.
//!
//! Le filtre ne tente pas d'interpréter le contenu des outils. Les résultats
//! d'outils sont sérialisés séparément par `runtime::tools::tool_history`.
//!
//! Les marqueurs de protocole sont traités comme une grammaire de flux :
//! lorsqu'un marqueur ou une fermeture de bloc est coupé entre deux chunks,
//! son suffixe partiel reste buffered et ne peut donc jamais fuiter dans le
//! contenu ACP.

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";
const TOOL_CALL_FENCE: &str = "```tool_call";
const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";
const THINKING_OPEN: &str = "<thinking>";
const THINKING_CLOSE: &str = "</thinking>";

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
    ThinkingBlock,
}

#[derive(Debug)]
pub struct OutputFilter {
    /// Partial protocol marker/fence carried across input chunks.
    candidate: String,
    at_line_start: bool,
    drop_mode: DropMode,
    skipping_marker_spacing: bool,
    suppress_protocol_newline: bool,
}

impl Default for OutputFilter {
    fn default() -> Self {
        Self {
            candidate: String::new(),
            at_line_start: true,
            drop_mode: DropMode::None,
            skipping_marker_spacing: false,
            suppress_protocol_newline: false,
        }
    }
}

impl OutputFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &str) -> String {
        self.process(chunk, false)
    }

    pub fn finish(&mut self) -> String {
        self.process("", true)
    }

    fn process(&mut self, chunk: &str, final_chunk: bool) -> String {
        let mut input = std::mem::take(&mut self.candidate);
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

                    // A tool-call fence is protocol syntax, not arbitrary
                    // markdown. Only a closing fence at line start terminates
                    // the block; this prevents backticks/quotes inside JSON,
                    // strings, or tool output from prematurely ending it.
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
                            self.candidate.push_str(&input[i..]);
                            break;
                        }
                    }

                    let ch = input[i..].chars().next().expect("index UTF-8 valide");
                    i += ch.len_utf8();
                    self.at_line_start = ch == '\n';
                    continue;
                }
                DropMode::ThinkingBlock => {
                    if let Some(end) = input[i..].find(THINKING_CLOSE) {
                        i += end + THINKING_CLOSE.len();
                        self.drop_mode = DropMode::None;
                        self.at_line_start = true;
                        self.suppress_protocol_newline = true;
                        continue;
                    }
                    let keep = partial_suffix_len(&input[i..], THINKING_CLOSE);
                    let end = input.len() - keep;
                    if end > i {
                        i = end;
                    }
                    if !final_chunk && i < input.len() {
                        self.candidate.push_str(&input[i..]);
                    }
                    break;
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
                    if !final_chunk {
                        return out;
                    }
                    break;
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
                if rest.starts_with(THINKING_OPEN) {
                    self.drop_mode = DropMode::ThinkingBlock;
                    i += THINKING_OPEN.len();
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
                    THINKING_OPEN,
                    ASSISTANT_MARKER,
                    USER_MARKER,
                ];
                if !final_chunk && prefixes.iter().any(|prefix| prefix.starts_with(rest)) {
                    self.candidate.push_str(rest);
                    break;
                }
            }

            let ch = input[i..].chars().next().expect("index UTF-8 valide");
            out.push(ch);
            self.at_line_start = ch == '\n';
            i += ch.len_utf8();
        }

        if final_chunk && !self.candidate.is_empty() {
            out.push_str(&self.candidate);
            self.candidate.clear();
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

pub fn sanitize_text(text: &str) -> String {
    let mut filter = OutputFilter::new();
    let mut out = filter.push(text);
    out.push_str(&filter.finish());
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_both_tool_result_protocol_forms() {
        assert_eq!(sanitize_text("[Tool result for shell_exec]: done"), "");
        assert_eq!(
            sanitize_text("[Tool result]: {\"tool\":\"file_read\",\"content\":\"done\"}"),
            ""
        );
    }

    #[test]
    fn preserves_arbitrary_tool_content_when_embedded_in_json() {
        let encoded = "[Tool result]: {\"tool\":\"file_read\",\"content\":\"line\\n[Assistant]: nope\\n'''\\n```\\n<thinking>secret</thinking>\"}";
        assert_eq!(sanitize_text(encoded), "");
    }

    #[test]
    fn removes_role_markers() {
        assert_eq!(
            sanitize_text("[Assistant]: J'exécute cargo check"),
            "J'exécute cargo check"
        );
        assert_eq!(
            sanitize_text("[User]: analyse le projet"),
            "analyse le projet"
        );
    }

    #[test]
    fn filters_marker_split_at_every_boundary() {
        let marker = "[Assistant]:";
        for split in 1..marker.len() {
            let mut filter = OutputFilter::new();
            assert_eq!(filter.push(&marker[..split]), "");
            assert_eq!(filter.push(&marker[split..]), "");
            assert_eq!(filter.push(" suite"), "suite");
            assert_eq!(filter.finish(), "");
        }
    }

    #[test]
    fn filters_tool_call_and_thinking_blocks() {
        let input = "<thinking>secret</thinking>\n```tool_call\n{}\n```\n[Tool result for glob]: []\n[Assistant]: Réponse finale";
        assert_eq!(sanitize_text(input), "Réponse finale");
    }

    #[test]
    fn filters_single_quote_tool_call_blocks() {
        let input = "'''tool_call\n{}\n'''[Tool result for glob]: []\n[Assistant]: Réponse";
        assert_eq!(sanitize_text(input), "Réponse");
    }

    #[test]
    fn preserves_single_quote_python_strings() {
        let input = "Voici du Python :\n'''docstring'''\nprint('ok')";
        assert_eq!(sanitize_text(input), input);
    }

    #[test]
    fn filters_tool_call_block_split_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("```tool_"), "");
        assert_eq!(filter.push("call\n{}\n```\n"), "");
        assert_eq!(filter.push("Réponse"), "Réponse");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_tool_call_closing_fence_split_at_every_boundary() {
        let closing = "```";
        for split in 1..closing.len() {
            let mut filter = OutputFilter::new();
            assert_eq!(filter.push("```tool_call\n{}\n"), "");
            assert_eq!(filter.push(&closing[..split]), "");
            assert_eq!(filter.push(&closing[split..]), "");
            assert_eq!(filter.push("\n[Assistant]: suite"), "suite");
            assert_eq!(filter.finish(), "");
        }
    }

    #[test]
    fn filters_single_quote_closing_fence_split_at_every_boundary() {
        let closing = "'''";
        for split in 1..closing.len() {
            let mut filter = OutputFilter::new();
            assert_eq!(filter.push("'''tool_call\n{}\n"), "");
            assert_eq!(filter.push(&closing[..split]), "");
            assert_eq!(filter.push(&closing[split..]), "");
            assert_eq!(filter.push("\n[Assistant]: suite"), "suite");
            assert_eq!(filter.finish(), "");
        }
    }

    #[test]
    fn ignores_fence_like_content_inside_tool_call_body() {
        let input = "```tool_call\n{\"value\":\"```\"}\n```\n[Assistant]: Réponse";
        assert_eq!(sanitize_text(input), "Réponse");
    }

    #[test]
    fn ignores_fence_like_content_inside_single_quote_tool_call_body() {
        let input = "'''tool_call\n{\"value\":\"'''\"}\n'''\n[Assistant]: Réponse";
        assert_eq!(sanitize_text(input), "Réponse");
    }

    #[test]
    fn preserves_normal_markdown_code_fence() {
        let input = "Voici un exemple :\n```rust\nfn main() {}\n```";
        assert_eq!(sanitize_text(input), input);
    }

    #[test]
    fn removes_tool_result_before_assistant_in_same_stream() {
        let mut filter = OutputFilter::new();
        assert_eq!(
            filter.push("[Tool result for shell_exec]: done\n[Assistant]: Suite"),
            "Suite"
        );
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn preserves_newlines_around_filtered_lines() {
        assert_eq!(
            sanitize_text("Avant\n[Tool result for shell_exec]: done\nAprès"),
            "Avant\nAprès"
        );
    }

    #[test]
    fn keeps_unclosed_tool_call_hidden_at_stream_end() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("```tool_call\n{\"secret\":true}\n"), "");
        assert_eq!(filter.finish(), "");
    }
}
