//! Gemini LLM provider implementation and its configuration primitives.
//!
//! Temporary source-compatibility aliases are intentionally local to this crate:
//! the implementation was renamed from `gemini_acp_config`, but its internal
//! modules still use that historical path. No workspace crate depends on this
//! alias.
extern crate self as gemini_acp_config;
extern crate agent_runtime as gemini_acp_runtime;

pub mod client;
pub mod config;
pub mod core;
pub mod provider;
pub mod settings;
pub mod web2api;

pub use client::{Client as GeminiClient, Config as ClientConfig};
pub use config::AgentConfig;
pub use core::models::{resolve as resolve_model, DEFAULT_MODEL};
pub use core::time::{now_iso, now_unix};
pub use core::{sapisid_hash, CookieJar, GeminiError, GeminiResult};
pub use provider::GeminiProvider;
pub use settings::{SettingsManager, SettingsManagerOptions};
