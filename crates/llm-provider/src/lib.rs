//! Gemini LLM provider implementation and its configuration primitives.

extern crate self as llm_provider;

pub mod client;
pub mod config;
pub mod core;
pub mod provider;
mod semantic_stream;
pub mod settings;
pub mod web2api;

pub use client::{Client as GeminiClient, Config as ClientConfig};
pub use config::AgentConfig;
pub use core::models::{resolve as resolve_model, DEFAULT_MODEL};
pub use core::time::{now_iso, now_unix};
pub use core::{sapisid_hash, CookieJar, GeminiError, GeminiResult};
pub use provider::GeminiProvider;
pub use settings::{SettingsManager, SettingsManagerOptions};
