//! Gemini LLM provider implementation and its configuration primitives.

pub mod client;
pub mod config;
pub mod core;
pub mod provider;
pub mod settings;
pub mod web2api;

/// Temporary local compatibility namespace for the historical implementation
/// paths used by the web2api port. It is internal to this crate and does not
/// recreate the removed workspace crate.
pub(crate) mod gemini_acp_config {
    pub(crate) use crate::client;
    pub(crate) use crate::core;
}

pub use client::{Client as GeminiClient, Config as ClientConfig};
pub use config::AgentConfig;
pub use core::models::{resolve as resolve_model, DEFAULT_MODEL};
pub use core::time::{now_iso, now_unix};
pub use core::{sapisid_hash, CookieJar, GeminiError, GeminiResult};
pub use provider::GeminiProvider;
pub use settings::{SettingsManager, SettingsManagerOptions};
