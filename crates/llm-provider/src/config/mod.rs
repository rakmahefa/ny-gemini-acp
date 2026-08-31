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
        // D-03 : le modèle par défaut est validé au chargement de la config —
        // une valeur inconnue retombe sur le défaut intégré (au lieu de
        // paniquer à chaque requête dans models::resolve).
        let default_model = env::env_or("GEMINI_ACP_MODEL", crate::core::models::DEFAULT_MODEL);
        let default_model = if crate::core::models::MODEL_KEYS.contains(&default_model.as_str()) {
            default_model
        } else {
            tracing::warn!(
                configured = %default_model,
                built_in = crate::core::models::DEFAULT_MODEL,
                "unknown GEMINI_ACP_MODEL, falling back to the built-in default model"
            );
            crate::core::models::DEFAULT_MODEL.to_string()
        };
        Self {
            cookie_file: env::env_or("GEMINI_ACP_COOKIES", "vendor/cookie.json").into(),
            default_model,
            data_dir: env::data_dir_default(),
            auth_user: env::parse_auth_user(),
            proxy: std::env::var("GEMINI_ACP_PROXY").ok(),
        }
    }

    pub fn validate(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();
        if !self.cookie_file.exists() {
            warnings.push(ConfigWarning(format!(
                "cookie file not found: {}",
                self.cookie_file.display()
            )));
        }
        if !crate::core::models::MODEL_KEYS.contains(&self.default_model.as_str()) {
            warnings.push(ConfigWarning(format!(
                "unknown default model: {} (valid keys: {})",
                self.default_model,
                crate::core::models::MODEL_KEYS.join(", ")
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
