//! Headless, deterministic core of the Turnbase engine.
//!
//! Async-free, UI-free, pure computation: state plus action to new state. See
//! `ARCHITECTURE.md` at the workspace root for the design and its rationale.

mod active;
mod chance;
mod effects;
mod error;
mod game;
mod player;
mod rng;
mod state;

#[cfg(all(test, feature = "serde"))]
mod serde_roundtrip;

pub use active::ActivePlayers;
pub use chance::sample_chance;
pub use effects::{EffectSystem, MAX_EFFECTS, resolve_effects};
pub use error::Error;
pub use game::{Game, Reversible};
pub use player::PlayerId;
pub use rng::Prng;
pub use state::{PlayerView, State};
