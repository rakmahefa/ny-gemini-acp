//! Filtrage de la sortie Gemini avant émission ACP.
//!
//! Ce module sépare explicitement deux responsabilités :
//! - `OutputFilter` traite les chunks du stream sans supposer que les marqueurs
//!   arrivent complets dans un seul chunk ;
//! - `sanitize_text` nettoie un texte déjà assemblé (historique / réponse finale).
//!
//! Les marqueurs ACP (`[Tool result for ...]`, `[Assistant]:`, `[User]:`) sont
//! des détails du protocole interne et ne doivent jamais atteindre
//! `AgentMessageChunk`.

const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
const ASSISTANT_MARKER: &str = "[Assistant]:";
const USER_MARKER: &str = "[User]:";

/// Filtre incrémental pour la sortie visible de Gemini.
#[derive(Default, Debug)]
pub struct OutputFilter {
    pending: String,
}

impl OutputFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume un chunk et retourne uniquement le texte sûr à afficher.
    ///
    /// Le filtre conserve la partie ambiguë en fin de chunk afin de gérer les
    /// marqueurs coupés entre deux événements réseau.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        self.drain(false)
    }

    /// Termine le stream et libère tout texte restant.
    pub fn finish(&mut self) -> String {
        self.drain(true)
    }

    fn drain(&mut self, final_chunk: bool) -> String {
        let mut out = String::new();

        loop {
            let Some(newline) = self.pending.find('\n') else {
                if final_chunk {
                    let line = std::mem::take(&mut self.pending);
                    out.push_str(&sanitize_line(&line));
                }
                break;
            };

            let line = self.pending[..newline].to_owned();
            self.pending.drain(..=newline);
            let sanitized = sanitize_line(&line);
            if !sanitized.is_empty() {
                out.push_str(&sanitized);
                out.push('\n');
            }
        }

        out
    }
}

/// Sanitize un texte déjà complet.
pub fn sanitize_text(text: &str) -> String {
    let mut filter = OutputFilter::new();
    let mut out = filter.push(text);
    out.push_str(&filter.finish());
    out.trim().to_owned()
}

fn sanitize_line(line: &str) -> String {
    let trimmed = line.trim_start();

    // Un résultat d'outil est toujours un bloc interne : toute la ligne est
    // supprimée, même si son contenu ressemble à du texte utilisateur.
    if trimmed.starts_with(TOOL_RESULT_PREFIX) {
        return String::new();
    }

    // Gemini peut recopier les rôles de l'historique. On retire uniquement le
    // marqueur et conservons le contenu utile qui suit.
    if let Some(rest) = trimmed.strip_prefix(ASSISTANT_MARKER) {
        return rest.trim_start().to_owned();
    }
    if let Some(rest) = trimmed.strip_prefix(USER_MARKER) {
        return rest.trim_start().to_owned();
    }

    line.to_owned()
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
    fn preserves_normal_text() {
        assert_eq!(sanitize_text("Compilation terminée."), "Compilation terminée.");
    }

    #[test]
    fn filters_stream_across_chunks() {
        let mut filter = OutputFilter::new();
        assert_eq!(filter.push("[Tool result for shell_"), "");
        assert_eq!(filter.push("exec]: Finished `dev` profile\n"), "");
        assert_eq!(filter.push("[Assistant]: Je lance cargo check\n"), "Je lance cargo check\n");
        assert_eq!(filter.finish(), "");
    }

    #[test]
    fn preserves_text_after_marker_on_same_line() {
        assert_eq!(
            sanitize_text("[Assistant]: J'exécute cargo check"),
            "J'exécute cargo check"
        );
    }

    #[test]
    fn removes_tool_result_before_assistant_in_same_stream() {
        assert_eq!(
            sanitize_text("[Tool result for shell_exec]: done\n[Assistant]: Suite de l'analyse"),
            "Suite de l'analyse"
        );
    }
}
