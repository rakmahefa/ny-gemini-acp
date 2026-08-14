//! ACP execution encapsulation.
//!
//! This crate owns the concurrency boundary around ACP work. It deliberately
//! does not own Gemini state, persistence, tools, or protocol handlers.

mod cancellation;
mod command;
mod error;
mod lifecycle;
mod thread;
mod turn;
mod turn_manager;

pub use cancellation::Cancellation;
pub use command::ThreadCommand;
pub use error::EncapsError;
pub use lifecycle::ThreadState;
pub use thread::{AcpThread, AcpThreadHandle};
pub use turn::{AcpTurn, AcpTurnHandle, TurnState};
pub use turn_manager::TurnManager;
