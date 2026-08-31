//! Gemini LLM provider implementation and its configuration primitives.

extern crate self as llm_provider;

pub mod client;
pub mod config;
pub mod core;
pub mod provider;
mod semantic_stream;

pub use config::AgentConfig;
pub use provider::GeminiProvider;
