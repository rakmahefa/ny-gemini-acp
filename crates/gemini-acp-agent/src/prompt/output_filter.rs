//! Filtrage de la sortie Gemini avant émission ACP.
//!
//! `OutputFilter` est volontairement indépendant de l'ACP : il transforme un
//! flux de texte Gemini en texte visible sûr. Il gère les marqueurs coupés
//! entre plusieurs chunks sans attendre la fin de la réponse normale.
//!
//! `sanitize_text` utilise le même moteur pour les textes déjà assemblés.

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";

/// Filtre incrémental pour la sortie visible de Gemini.
#[derive(Debug)]
pub struct OutputFilter {
    candidate: String,
    at_line_start: bool,
    dropping_tool_result: bool,
}

impl Default for OutputFilter {
    fn default() -> Self {
        Self {
            candidate: String::new(),
            at_line_start: true,
            dropping_tool_result: false,
        }
    }
}

impl OutputFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume un chunk et retourne immédiatement le texte sûr à afficher.
    ///
    /// Seule une petite séquence potentiellement ambiguë est retenue lorsqu'un
    /// marqueur commence à la frontière du chunk. Le texte normal n'est donc
    /// pas bloqué jusqu'à la fin du stream.
    pub fn push(&mut self, chunk: &str) -> String {
        self.process(chunk, false)
    }

    /// Termine le stream et libère tout candidat partiellement reconnu.
    pub fn finish(&mut self) -> String {
        self.process("", true)
    }

    fn process(&mut self, chunk: &str, final_chunk: bool) -> String {
        let mut input = self.candidate.clone();
        self.candidate.clear();
        input.push_str(chunk);
        let mut out = String::new();
        let bytes = input.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if self.dropping_tool_result {
                if bytes[i] == b'\n' {
                    self.dropping_tool_result = false;
                    self.at_line_start = true;
                }
                i += 1;
                continue;
            }

            if self.at_line_start {
                let rest = &input[i..];
                if rest.starts_with(TOOL_RESULT_PREFIX) {
                    self.dropping_tool_result = true;
                    i += TOOL_RESULT_PREFIX.len();
                    continue;
                }
                if rest.starts_with(ASSISTANT_MARKER) {
                    i += ASSISTANT_MARKER.len();
                    self.at_line_start = false;
                    continue;
                }
                if rest.starts_with(USER_MARKER) {
                    i += USER_MARKER.len();
                    self.at_line_start = false;
                    continue;
                }

                // Si le chunk peut encore devenir un protocole marker, on
                // attend le prochain chunk au lieu de l'émettre partiellement.
                let prefixes = [TOOL_RESULT_PREFIX, ASSISTANT_MARKER, USER_MARKER];
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
    fn preserves_normal_text_immediately() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("Je lance "), "Je lance ");
        assert_eq!(filter.push("cargo check"), "cargo check");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_marker_split_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("[Tool result for shell_"), "");
        assert_eq!(filter.push("exec]: Finished `dev` profile\n"), "");
        assert_eq!(filter.push("[Assistant]: Je lance cargo check"), "Je lance cargo check");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn filters_marker_split_at_every_boundary() {
        let marker = "[Assistant]:";
        for split in 1..marker.len() {
            let mut filter = OutputFilter::new();
            assert_eq!(filter.push(&marker[..split]), "");
            assert_eq!(filter.push(&marker[split..]), "");
            assert_eq!(filter.push(" suite"), " suite");
            assert_eq!(filter.finish(), "");
        }
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
}
