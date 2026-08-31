//! Matching glob minimaliste du workspace (C-31/P-11 : une seule
//! implémentation, partagée par `builtin::filesystem` et `builtin::search`).
//!
//! Sémantique : `*` correspond à tout sauf `/`, `**` correspond à tout,
//! `?` correspond à un caractère quelconque sauf `/`.

use regex::Regex;

pub(crate) fn glob_to_regex(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut regex = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                regex.push_str(".*");
                i += 2;
            }
            '*' => {
                regex.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                regex.push_str("[^/]");
                i += 1;
            }
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                regex.push('\\');
                regex.push(chars[i]);
                i += 1;
            }
            c => {
                regex.push(c);
                i += 1;
            }
        }
    }
    regex.push('$');
    regex
}

/// Matching d'une chaîne candidate (chemin normalisé ou basename) contre un glob.
pub(crate) fn glob_matches(pattern: &str, candidate: &str) -> bool {
    Regex::new(&glob_to_regex(pattern))
        .map(|re| re.is_match(candidate))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{glob_matches, glob_to_regex};

    #[test]
    fn glob_semantics_are_preserved() {
        assert!(glob_matches("*.rs", "main.rs"));
        assert!(!glob_matches("*.rs", "src/main.rs"));
        assert!(glob_matches("**/*.rs", "src/main.rs"));
        assert!(glob_matches("src/?s", "src/rs"));
        let re = glob_to_regex("a.b");
        assert!(regex::Regex::new(&re).unwrap().is_match("a.b"));
        assert!(!regex::Regex::new(&re).unwrap().is_match("aXb"));
    }
}
