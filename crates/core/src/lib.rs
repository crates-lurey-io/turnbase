//! Headless, deterministic core of the Turnbase engine.
//!
//! Async-free, UI-free, pure computation: state plus action to new state. See
//! `ARCHITECTURE.md` at the workspace root for the design and its rationale.

mod active;
mod error;
mod player;
mod rng;

pub use active::ActivePlayers;
pub use error::Error;
pub use player::PlayerId;
pub use rng::Prng;
