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

/// Filtre incrémental pour la sortie visible de Gemini.
#[derive(Debug)]
pub struct OutputFilter {
    candidate: String,
    at_line_start: bool,
    drop_mode: DropMode,
    skipping_marker_spacing: bool,
}

impl Default for OutputFilter {
    fn default() -> Self {
        Self {
            candidate: String::new(),
            at_line_start: true,
            drop_mode: DropMode::None,
            skipping_marker_spacing: false,
        }
    }
}

impl OutputFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume un chunk et retourne uniquement le texte sûr à afficher.
    pub fn push(&mut self, chunk: &str) -> String {
        self.process(chunk, false)
    }

    /// Termine le stream et libère un éventuel candidat partiellement reconnu.
    pub fn finish(&mut self) -> String {
        self.process("", true)
    }

    fn process(&mut self, chunk: &str, final_chunk: bool) -> String {
        let mut input = std::mem::take(&mut self.candidate);
        input.push_str(chunk);
        let mut out = String::new();
        let mut i = 0;

        while i < input.len() {
            if self.drop_mode != DropMode::None {
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
                        if let Some(len) = line_prefix_len(&input[i..], "```") {
                            i += len;
                            self.drop_mode = DropMode::None;
                            self.at_line_start = false;
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
                            self.at_line_start = false;
                            continue;
                        }
                        // La fermeture peut être coupée entre deux chunks.
                        let keep = partial_suffix_len(&input[i..], THINKING_CLOSE);
                        let end = input.len() - keep;
                        if end > i {
                            self.at_line_start = input[..end].ends_with('\n');
                            i = end;
                        }
                        if !final_chunk && i < input.len() {
                            self.candidate.push_str(&input[i..]);
                        }
                        break;
                    }
                    DropMode::None => unreachable!(),
                }
                continue;
            }

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

                if let Some(len) = complete_or_partial(rest, TOOL_RESULT_PREFIX, final_chunk) {
                    if len == TOOL_RESULT_PREFIX.len() {
                        self.drop_mode = DropMode::ToolResultLine;
                        i += len;
                        continue;
                    }
                    self.candidate.push_str(rest);
                    break;
                }

                if let Some(len) = complete_or_partial(rest, TOOL_CALL_FENCE, final_chunk) {
                    if len == TOOL_CALL_FENCE.len() {
                        self.drop_mode = DropMode::ToolCallBlock;
                        i += len;
                        continue;
                    }
                    self.candidate.push_str(rest);
                    break;
                }

                if let Some(len) = complete_or_partial(rest, THINKING_OPEN, final_chunk) {
                    if len == THINKING_OPEN.len() {
                        self.drop_mode = DropMode::ThinkingBlock;
                        i += len;
                        continue;
                    }
                    self.candidate.push_str(rest);
                    break;
                }

                if let Some(len) = complete_or_partial(rest, ASSISTANT_MARKER, final_chunk) {
                    if len == ASSISTANT_MARKER.len() {
                        i += len;
                        self.skipping_marker_spacing = true;
                        continue;
                    }
                    self.candidate.push_str(rest);
                    break;
                }

                if let Some(len) = complete_or_partial(rest, USER_MARKER, final_chunk) {
                    if len == USER_MARKER.len() {
                        i += len;
                        self.skipping_marker_spacing = true;
                        continue;
                    }
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

/// Retourne la longueur du préfixe complet si `text` commence par `prefix`,
/// ou la longueur totale de `text` lorsqu'il s'agit d'un début de préfixe.
fn complete_or_partial(text: &str, prefix: &str, final_chunk: bool) -> Option<usize> {
    if text.starts_with(prefix) {
        return Some(prefix.len());
    }
    if !final_chunk && prefix.starts_with(text) {
        return Some(text.len());
    }
    None
}

fn line_prefix_len(text: &str, prefix: &str) -> Option<usize> {
    if text.starts_with(prefix) && (text.len() == prefix.len() || text.as_bytes()[prefix.len()] == b'\n') {
        Some(prefix.len())
    } else {
        None
    }
}

/// Nombre d'octets du suffixe de `text` qui peut encore être le début de
/// `needle`. Cela permet de retenir uniquement la partie ambiguë à la frontière
/// d'un chunk.
fn partial_suffix_len(text: &str, needle: &str) -> usize {
    let max = text.len().min(needle.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&needle[..len]) {
            return len;
        }
    }
    0
}

/// Nettoie un texte déjà assemblé avec exactement les mêmes règles que le
/// filtre streaming.
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
    fn filters_thinking_close_split_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("<thinking>secret</think"), "");
        assert_eq!(filter.push("ing>\nRéponse"), "\nRéponse");
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
