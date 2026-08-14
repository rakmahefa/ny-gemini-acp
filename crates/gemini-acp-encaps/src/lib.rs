//! ACP execution encapsulation.
//!
//! This crate owns the concurrency boundary around ACP work. It deliberately
//! does not own Gemini state, persistence, tools, or protocol handlers.
//!
//! # Public invariants
//!
//! - [`AcpThread`] is a single-use execution object: `start` is valid only
//!   from `ThreadState::Created` and a terminal thread cannot be restarted.
//! - Thread shutdown is request-based. `stop` is idempotent and signals
//!   cancellation; the worker owns the final transition to `Stopped` or
//!   `Failed`.
//! - [`TurnManager`] permits at most one active turn reservation per session.
//!   A competing `start` fails with [`EncapsError::TurnAlreadyActive`] before
//!   its worker is spawned.
//! - Turn cancellation is safe to race with normal completion; cancelling a
//!   session with no active turn simply returns `false`.
//! - The encapsulation layer is protocol-agnostic: Gemini/runtime code is
//!   supplied as closures and is not embedded in this crate.

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
