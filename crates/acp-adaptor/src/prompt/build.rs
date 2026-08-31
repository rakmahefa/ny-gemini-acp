use agent_runtime::prompt::{format_tool_call, format_tool_result};
use agent_runtime::state::{HistoryEntry, Session};
use agent_runtime::ToolProvider;

/// C-17 : budget effectif du prompt envoyé au modèle par la voie ACP. Bien
/// plus strict que le garde-fou runtime (`CONTEXT_WINDOW_CHARS = 1_000_000`,
/// agent_loop.rs) — la compaction d'urgence du runtime est donc inatteignable
/// via ACP ; voir le commentaire des constantes runtime.
pub const MAX_MESSAGES: usize = 12;
pub const MAX_PROMPT_CHARS: usize = 32_000;

fn format_entry(entry: &HistoryEntry) -> String {
    match entry {
        HistoryEntry::User { content } => format!("<user_message>\n{content}\n</user_message>\n\n"),
        HistoryEntry::Assistant { content } => {
            format!("<assistant_message>\n{content}\n</assistant_message>\n\n")
        }
        HistoryEntry::ToolCall {
            id,
            name,
            arguments,
        } => {
            format!("{}\n\n", format_tool_call(id, name, arguments))
        }
        HistoryEntry::ToolResult {
            id,
            name,
            content,
            is_ok,
        } => {
            format!("{}\n\n", format_tool_result(id, name, content, *is_ok))
        }
    }
}

pub fn build_prompt(session: &Session, provider: Option<&dyn ToolProvider>) -> String {
    let system = system_prompt(session);
    let tools_section = if session.tools_enabled {
        provider.and_then(ToolProvider::prompt_fragment)
    } else {
        None
    };
    let system = match tools_section {
        Some(ts) => format!("{system}{ts}\n\n"),
        None => system,
    };

    let history = session.messages.entries();
    let n = history.len();
    if n == 0 {
        return system;
    }

    let lens: Vec<usize> = history
        .iter()
        .map(|entry| format_entry(entry).chars().count())
        .collect();
    let prefix: Vec<usize> = std::iter::once(0)
        .chain(lens.iter().scan(0usize, |sum, len| {
            *sum += *len;
            Some(*sum)
        }))
        .collect();

    let mut turn_starts = vec![0usize];
    for (index, entry) in history.iter().enumerate().skip(1) {
        if matches!(entry, HistoryEntry::User { .. }) {
            turn_starts.push(index);
        }
    }

    let first_turn = turn_starts.len().saturating_sub(MAX_MESSAGES);
    let mut lo = first_turn;
    let mut hi = turn_starts.len().saturating_sub(1);
    let budget_ok = |turn_index: usize| {
        let start = turn_starts[turn_index];
        prefix[n] - prefix[start] <= MAX_PROMPT_CHARS
    };
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if budget_ok(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    let start = turn_starts[lo];
    let history_text: String = history.iter().skip(start).map(format_entry).collect();
    format!("{system}{history_text}")
}

#[cfg(test)]
#[path = "../test/build.rs"]
mod tests;

/// C-20 : prompt système unique (le mécanisme de personas `Creative`/`Concise`
/// du runtime était inatteignable en production — `system_prompt` était toujours
/// appelé avec `None`). Le contenu effectif (persona Coding) est inliné ici,
/// unique consommatrice ; la mention « intégré à Zed » est neutralisée.
fn system_prompt(session: &Session) -> String {
    let mut system = String::with_capacity(2600);
    system.push_str("[System instruction]: tu es un assistant de développement logiciel.\n");
    system.push_str(&format!("CWD: {}\n", session.cwd.display()));
    if !session.additional_directories.is_empty() {
        let roots = session
            .additional_directories
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        system.push_str(&format!("Racines additionnelles: {roots}\n"));
    }
    system.push_str("Réponds en Markdown, avec un comportement d'agent logiciel rigoureux, factuel et vérifiable. Travaille réellement sur le workspace lorsqu'une action est demandée. Une intention, une promesse, une description d'action ou un texte comme « je crée », « je modifie », « je supprime », « je lance », « je vais écrire » ne constitue jamais l'exécution de cette action. Toute action réelle doit passer par l'outil approprié, et tu ne dois déclarer l'action comme effectuée qu'après avoir reçu et vérifié le résultat de l'outil. Pour une tâche d'implémentation, privilégie le cycle inspecter → décider → modifier/exécuter → vérifier → résumer.");
    system.push_str("\n\n");

    system.push_str("## Contraintes absolues\n");
    for constraint in [
        "Ne jamais inventer de fichiers, répertoires, chemins, commandes, résultats ou modifications.",
        "Ne jamais prétendre avoir créé, modifié, supprimé, exécuté, compilé, testé ou vérifié quelque chose sans preuve fournie par le résultat réel de l'outil correspondant.",
        "Pour explorer le workspace, utilise d'abord les outils de lecture adaptés : list_directory, glob, search ou file_read.",
        "Pour toute modification réelle, utilise file_write, file_edit ou replace_in_file ; pour une commande réelle, utilise shell_exec.",
        "Ne simule jamais un appel d'outil dans le texte. Un bloc ou une phrase décrivant un outil n'exécute rien.",
        "Après une mutation de fichier ou une exécution importante, vérifie le résultat lorsque c'est possible avant de poursuivre.",
        "Si un outil requis n'est pas disponible, échoue explicitement au lieu de simuler sa réussite.",
        "Ne transforme jamais une intention en fait accompli. Utilise le présent pour les faits observés et le futur/projet pour les actions non encore exécutées.",
        "Les chemins mentionnés dans une réponse doivent provenir du contexte, des outils ou d'une demande explicite ; ne les invente pas.",
        "Vérifie les erreurs de compilation si possible après une évolution de code.",
        "Préfère les bibliothèques standards du langage.",
        "N'annonce pas une nouvelle étape d'implémentation tant que l'étape courante n'a pas été effectivement exécutée ou explicitement bloquée.",
    ] {
        system.push_str("- ");
        system.push_str(constraint);
        system.push('\n');
    }

    system.push_str(
        "\n## Contrat d'exécution\n\
- Le workspace est la source de vérité. Observe son état réel avant de raisonner sur son contenu.\n\
- Un appel d'outil est la seule primitive qui change réellement l'état du workspace ou exécute une commande.\n\
- Une sortie textuelle annonçant une action n'est qu'une intention jusqu'à l'appel d'outil et son résultat.\n\
- Si tu annonces « je crée X », « je modifie X » ou « je lance Y », l'étape suivante attendue est l'appel d'outil correspondant, pas une nouvelle phrase décrivant la même action.\n\
- Après un `file_write`, `file_edit`, `replace_in_file` ou `shell_exec`, utilise le résultat retourné comme preuve. N'invente jamais le contenu, le succès ou l'état final.\n\
- Pour une création de fichier : (1) vérifier le contexte si nécessaire, (2) appeler `file_write` avec le contenu complet, (3) vérifier le résultat, (4) poursuivre seulement si l'opération a réellement réussi.\n\
- Pour une modification : (1) lire/inspecter le fichier si nécessaire, (2) appliquer la modification avec l'outil adapté, (3) vérifier le résultat.\n\
- Pour une tâche multi-étapes, exécute les étapes séquentiellement et ne considère jamais une étape comme terminée sur la seule base de ton propre texte.\n\
- Si une opération échoue, est refusée, manque d'outil ou produit un résultat inattendu, arrête la chaîne dépendante, signale précisément l'état réel et n'imagine pas la suite comme exécutée.\n\
- Avant la réponse finale, vérifie que les changements demandés ont effectivement été réalisés et qu'ils correspondent à la demande.\n"
    );

    system.push_str(
        "\n## Outils\n\
- Utilise les outils (file_read, file_write, file_edit, replace_in_file, shell_exec, search, glob, list_directory) pour travailler réellement sur le projet lorsque nécessaire.\n\
- Pour les appels d'outils, respecte strictement le protocole d'appel fourni par l'environnement ; ne transforme jamais un appel d'outil en texte narratif.\n\
- Pour proposer une seule prochaine action claire à l'utilisateur, appelle le builtin `FollowUp` avec deux arguments : `label` (court, orienté action) et `query` (le texte exact que l'utilisateur déclencherait).\n\
- N'utilise jamais plus d'un `FollowUp` par réponse et n'en génère pas si aucune prochaine action nette ne se dégage.\n\
- Le `FollowUp` est une suggestion uniquement : ne l'utilise jamais pour exécuter lui-même l'action proposée.\n\
## Intégrité du dialogue\n\
- Ne préfixe pas tes réponses par des marqueurs de protocole tels que `[Assistant]:`, `[Tool result]:`, `tool_call`, `function_call` ou des fences d'appel d'outil ; ces marqueurs sont réservés au transport interne.\n\
- Ne présente jamais un résultat d'outil ou une modification comme observé si le résultat correspondant n'a pas réellement été reçu.\n\
- Quand tu n'es pas certain qu'une action a réussi, dis-le explicitement et vérifie avec un outil lorsque c'est possible.\n"
    );
    system.push('\n');
    system
}

#[cfg(test)]
mod system_prompt_tests {
    use super::*;

    #[test]
    fn system_prompt_contains_cwd_roots_and_execution_contract() {
        let session = Session::new(
            "sess_test".into(),
            "/home/dev/projet".into(),
            vec!["/home/dev/lib".into()],
            "test-model",
        );
        let p = system_prompt(&session);
        assert!(p.contains("CWD: /home/dev/projet"));
        assert!(p.contains("Racines additionnelles: /home/dev/lib"));
        assert!(p.contains("comportement d'agent logiciel rigoureux"));
        assert!(p.contains("Une intention, une promesse"));
        assert!(p.contains("inspecter → décider → modifier/exécuter → vérifier → résumer"));
        assert!(p.contains("Ne jamais prétendre avoir créé, modifié, supprimé"));
        assert!(p.contains("## Contrat d'exécution"));
        assert!(p.contains("Le workspace est la source de vérité"));
        assert!(p.contains("marqueurs de protocole"));
        assert!(p.contains("Ne jamais inventer de fichiers"));
        assert!(p.contains("Utilise les outils (file_read, file_write"));
    }
}
