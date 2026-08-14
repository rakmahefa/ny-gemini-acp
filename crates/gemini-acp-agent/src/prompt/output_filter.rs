//! Filtrage de la sortie Gemini avant émission ACP.
//!
//! `OutputFilter` est une machine d'état incrémentale. Elle protège la sortie
//! visible contre les artefacts du protocole que Gemini peut réémettre :
//! marqueurs de rôle, résultats d'outils, blocs `tool_call` et blocs
//! `<thinking>`. L'état est conservé entre les chunks afin qu'une frontière
//! de streaming au milieu d'un marqueur ne puisse provoquer de fuite.

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";
const TOOL_CALL_FENCE: &str = "```tool_call";
const THINKING_OPEN: &str = "<thinking>";
const THINKING_CLOSE: &str = "</thinking>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropMode {
    None,
    ToolResultLine,
    ToolCallBlock,
    ThinkingBlock,
}

#[derive(Debug)]
pub struct OutputFilter {
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
                DropMode::ToolCallBlock => {
                    if input[i..].starts_with("```") {
                        i += 3;
                        self.drop_mode = DropMode::None;
                        // The closing fence belongs to the protocol. Treat the
                        // following bytes as a fresh line so a tool result can
                        // immediately follow it without leaking.
                        self.at_line_start = true;
                        self.suppress_protocol_newline = true;
                        continue;
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
                if rest.starts_with(TOOL_RESULT_PREFIX) {
                    self.drop_mode = DropMode::ToolResultLine;
                    i += TOOL_RESULT_PREFIX.len();
                    continue;
                }
                if rest.starts_with(TOOL_CALL_FENCE) {
                    self.drop_mode = DropMode::ToolCallBlock;
                    i += TOOL_CALL_FENCE.len();
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
                    TOOL_CALL_FENCE,
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
            let len = ch.len_utf8();
            out.push(ch);
            self.at_line_start = ch == '\n';
            i += len;
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
    fn removes_tool_result_line() {
        assert_eq!(sanitize_text("[Tool result for shell_exec]: Finished `dev` profile"), "");
    }

    #[test]
    fn removes_role_marker_but_preserves_content() {
        assert_eq!(sanitize_text("[Assistant]: J'exécute cargo check"), "J'exécute cargo check");
        assert_eq!(sanitize_text("[User]: analyse le projet"), "analyse le projet");
    }

    #[test]
    fn removes_role_marker_with_multiple_spaces() {
        assert_eq!(sanitize_text("[Assistant]:   J'exécute cargo check"), "J'exécute cargo check");
        assert_eq!(sanitize_text("[User]:\t analyse le projet"), "analyse le projet");
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
        let input = "<thinking>secret</thinking>\n```tool_call\n{\"name\":\"glob\"}\n```\n[Tool result for glob]: []\n[Assistant]: Réponse finale";
        assert_eq!(sanitize_text(input), "Réponse finale");
    }

    #[test]
    fn filters_tool_call_followed_immediately_by_tool_result() {
        let input = "```tool_call\n{\"name\":\"glob\"}\n```[Tool result for glob]: []\n[Assistant]: Réponse";
        assert_eq!(sanitize_text(input), "Réponse");
    }

    #[test]
    fn filters_the_reported_leak() {
        let input = r#"<thinking>
```tool_call
{"id":"gemini_call_1","name":"file_read"}
```
```tool_call
{"id":"gemini_call_2","name":"glob"}
```[Tool result for file_read]: erreur
[Tool result for glob]: []
[Assistant]: ```tool_call
{"name":"glob"}
```
[Tool result for glob]: []
[Assistant]: Le fichier n'existe pas.
</thinking>"#;
        assert_eq!(sanitize_text(input), "");
    }

    #[test]
    fn filters_tool_call_block_split_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("```tool_"), "");
        assert_eq!(filter.push("call\n{\"name\":\"glob\"}\n```\n"), "");
        assert_eq!(filter.push("Réponse"), "Réponse");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_tool_result_after_tool_call_without_newline() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("```tool_call\n{}\n```"), "");
        assert_eq!(filter.push("[Tool result for glob]: []\n[Assistant]: Réponse"), "Réponse");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_thinking_close_split_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("<thinking>secret</think"), "");
        assert_eq!(filter.push("ing>\nRéponse"), "Réponse");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn preserves_normal_markdown_code_fence() {
        let input = "Voici un exemple :\n```rust\nfn main() {}\n```";
        assert_eq!(sanitize_text(input), input);
    }

    #[test]
    fn preserves_normal_text_immediately() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("Je lance "), "Je lance ");
        assert_eq!(filter.push("cargo check"), "cargo check");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn removes_tool_result_before_assistant_in_same_stream() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("[Tool result for shell_exec]: done\n[Assistant]: Suite"), "Suite");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn preserves_newlines_around_filtered_lines() {
        assert_eq!(sanitize_text("Avant\n[Tool result for shell_exec]: done\nAprès"), "Avant\nAprès");
    }
}
