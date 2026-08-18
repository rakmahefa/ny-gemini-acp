//! Provider-local Gemini core utilities.
//!
//! These modules implement Gemini authentication, cookies, framing, models,
//! tool prompt helpers and time utilities. They are not runtime contracts.
pub mod auth;
pub mod cookies;
pub mod errors;
pub mod frames;
pub mod models;
pub mod time;
pub mod tool_prompt;

pub use auth::sapisid_hash;
pub use cookies::CookieJar;
pub use errors::{GeminiError, GeminiResult};
