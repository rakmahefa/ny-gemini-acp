//! Gemini LLM provider implementation and its configuration primitives.

pub mod client;
pub mod config;
pub mod core;
pub mod provider;
pub mod settings;
pub mod web2api;

/// Temporary local compatibility namespace for historical implementation
/// paths still used by the web2api port. It is internal to this crate and does
/// not recreate the removed workspace crate.
pub(crate) mod gemini_acp_config {
    pub(crate) use crate::client;
    pub(crate) use crate::core;
}

/// Temporary local name for the provider-neutral runtime dependency. This is
/// only a source-compatibility alias used by the Gemini provider implementation.
pub(crate) use agent_runtime as gemini_acp_runtime;

pub use client::{Client as GeminiClient, Config as ClientConfig};
pub use config::AgentConfig;
pub use core::models::{resolve as resolve_model, DEFAULT_MODEL};
pub use core::time::{now_iso, now_unix};
pub use core::{sapisid_hash, CookieJar, GeminiError, GeminiResult};
pub use provider::GeminiProvider;
pub use settings::{SettingsManager, SettingsManagerOptions};
