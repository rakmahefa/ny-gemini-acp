//! Configuration de la face API (spec §5.1).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use llm_provider::client::DEFAULT_BL;
use llm_provider::core::models::DEFAULT_MODEL;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    pub host: String,
    pub retry_attempts: u32,
    pub retry_delay_sec: u64,
    pub request_timeout_sec: u64,
    pub gemini_bl: String,
    pub auth_user: Option<u32>,
    pub xsrf_token: Option<String>,
    pub default_model: String,
    pub log_requests: bool,
    pub cookie_file: Option<PathBuf>,
    pub proxy: Option<String>,
    pub api_keys: Vec<String>,
    pub cors_origins: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8081,
            host: "127.0.0.1".into(),
            retry_attempts: 3,
            retry_delay_sec: 2,
            request_timeout_sec: 180,
            gemini_bl: DEFAULT_BL.to_string(),
            auth_user: None,
            xsrf_token: None,
            default_model: DEFAULT_MODEL.to_string(),
            log_requests: true,
            cookie_file: None,
            proxy: None,
            api_keys: Vec::new(),
            cors_origins: Vec::new(),
        }
    }
}

fn config_paths() -> [PathBuf; 2] {
    // Apply the shared user config first and the project-local config second,
    // so the local file intentionally overrides global defaults.
    [
        dirs_config().join("gemini-web2api/config.json"),
        PathBuf::from("./config.json"),
    ]
}

fn dirs_config() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join(".config"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load_from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Config {
    let mut config = Config::default();
    for path in paths {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<Config>(&raw) {
                Ok(from_file) => {
                    config = merge_config(config, from_file);
                    tracing::info!("config loaded from {}", path.display());
                }
                Err(error) => tracing::warn!("invalid config.json ({}): {error}", path.display()),
            }
        }
    }
    config
}

// Keep this explicit: a file may specify only a subset of fields, and should
// override those fields without erasing values from the preceding layer.
fn merge_config(mut base: Config, override_with: Config) -> Config {
    base.port = override_with.port;
    base.host = override_with.host;
    base.retry_attempts = override_with.retry_attempts;
    base.retry_delay_sec = override_with.retry_delay_sec;
    base.request_timeout_sec = override_with.request_timeout_sec;
    base.gemini_bl = override_with.gemini_bl;
    base.auth_user = override_with.auth_user;
    base.xsrf_token = override_with.xsrf_token;
    base.default_model = override_with.default_model;
    base.log_requests = override_with.log_requests;
    base.cookie_file = override_with.cookie_file;
    base.proxy = override_with.proxy;
    base.api_keys = override_with.api_keys;
    base.cors_origins = override_with.cors_origins;
    base
}

pub fn load() -> Config {
    let mut config = load_from_paths(config_paths());
    apply_env(&mut config);
    config
}

fn apply_env(config: &mut Config) {
    let get = |key: &str| std::env::var(format!("GEMINI_WEB2API_{key}")).ok();
    if let Some(v) = get("PORT") {
        if let Ok(n) = v.parse() {
            config.port = n;
        }
    }
    if let Some(v) = get("HOST") {
        config.host = v;
    }
    if let Some(v) = get("RETRY_ATTEMPTS") {
        if let Ok(n) = v.parse() {
            config.retry_attempts = n;
        }
    }
    if let Some(v) = get("RETRY_DELAY_SEC") {
        if let Ok(n) = v.parse() {
            config.retry_delay_sec = n;
        }
    }
    if let Some(v) = get("REQUEST_TIMEOUT_SEC") {
        if let Ok(n) = v.parse() {
            config.request_timeout_sec = n;
        }
    }
    if let Some(v) = get("GEMINI_BL") {
        config.gemini_bl = v;
    }
    if let Some(v) = get("AUTH_USER") {
        config.auth_user = v.parse().ok();
    }
    if let Some(v) = get("XSRF_TOKEN") {
        config.xsrf_token = Some(v);
    }
    if let Some(v) = get("DEFAULT_MODEL") {
        config.default_model = v;
    }
    if let Some(v) = get("LOG_REQUESTS") {
        config.log_requests = v != "0" && v != "false";
    }
    if let Some(v) = get("COOKIE_FILE") {
        config.cookie_file = Some(v.into());
    }
    if let Some(v) = get("PROXY") {
        config.proxy = Some(v);
    }
    if let Some(v) = get("API_KEYS") {
        config.api_keys = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(v) = get("CORS_ORIGINS") {
        config.cors_origins = v
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(str::to_owned)
            .collect();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_config_overrides_global_config() {
        let root = std::env::temp_dir().join(format!(
            "gemini-web2api-config-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let global = root.join("global.json");
        let local = root.join("local.json");
        std::fs::write(&global, r#"{"host":"0.0.0.0","port":9000}"#).unwrap();
        std::fs::write(&local, r#"{"host":"127.0.0.1","port":8123}"#).unwrap();

        let config = load_from_paths([global.clone(), local.clone()]);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8123);

        std::fs::remove_dir_all(root).unwrap();
    }
}
