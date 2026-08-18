//! Configuration de l'agent — résolution provider-local.
//!
//! La configuration est résolue une seule fois (`AgentConfig::from_env`), puis
//! injectée vers les couches supérieures. Aucun contrat runtime ne dépend de
//! l'environnement directement.
pub mod env;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub cookie_file: PathBuf,
    pub default_model: String,
    pub data_dir: PathBuf,
    pub auth_user: Option<u32>,
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWarning(pub String);

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AgentConfig {
    pub fn from_env() -> Self {
        Self {
            cookie_file: env::env_or("GEMINI_ACP_COOKIES", "vendor/cookie.json").into(),
            default_model: env::env_or("GEMINI_ACP_MODEL", crate::core::models::DEFAULT_MODEL),
            data_dir: env::data_dir_default(),
            auth_user: env::parse_auth_user(),
            proxy: std::env::var("GEMINI_ACP_PROXY").ok(),
        }
    }

    pub fn validate(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();
        if !self.cookie_file.exists() {
            warnings.push(ConfigWarning(format!(
                "fichier de cookies introuvable: {}",
                self.cookie_file.display()
            )));
        }
        warnings
    }
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[path = "../test/config.rs"]
mod tests;
