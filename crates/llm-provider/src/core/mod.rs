//! Provider-local Gemini core utilities.
//!
//! Cookies, authentication, response framing, model resolution, typed errors
//! and time helpers remain implementation details of the Gemini provider.
pub mod auth;
pub mod cookies;
pub mod errors;
pub mod frames;
pub mod models;
pub mod tool_prompt;

pub use auth::sapisid_hash;
pub use cookies::CookieJar;
pub use errors::{GeminiError, GeminiResult};
pub use models::resolve as resolve_model;
