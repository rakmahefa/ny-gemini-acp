//! Dérivation automatique du titre de session.
//!
//! C-29 : un seul système de titre dans le workspace — la dérivation
//! (première ligne du message) reste ici, mais la normalisation et la limite
//! sont celles de `agent_runtime::SessionManager::sanitize_title`
//! (`MAX_TITLE_LENGTH = 256`), seule constante de troncature de titre.

pub fn derive_title(first_user_message: &str) -> String {
    let trimmed = first_user_message.trim();
    let single_line = trimmed.split('\n').next().unwrap_or("").trim();
    if single_line.is_empty() {
        return "Nouvelle session".to_string();
    }
    agent_runtime::SessionManager::sanitize_title(single_line)
        .unwrap_or_else(|| "Nouvelle session".to_string())
}

#[cfg(test)]
#[path = "../test/title.rs"]
mod tests;
