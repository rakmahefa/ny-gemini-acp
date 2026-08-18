//! `llm-provider` — provider contract plus Gemini implementation.
//!
//! The crate contains the provider-neutral LLM boundary and the Gemini-specific
//! transport/configuration underneath it. Agent runtime code consumes the
//! `LlmProvider` contract rather than Gemini wire types.

pub mod client;
pub mod config;
pub mod core;
pub mod llm;
pub mod settings;

pub use client::{Client as GeminiClient, Config as ClientConfig};
pub use config::AgentConfig;
pub use core::models::{resolve as resolve_model, DEFAULT_MODEL};
pub use core::time::{now_iso, now_unix};
pub use core::{sapisid_hash, CookieJar, GeminiError, GeminiResult};
pub use llm::{LlmError, LlmProvider, LlmRequest, LlmStream};
pub use settings::{SettingsManager, SettingsManagerOptions};
