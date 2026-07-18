//! Turn-based game loop coordination for [`turnbase`], with an optional
//! [`retroglyph`](retroglyph_core) terminal UI.
//!
//! The simulation core ([`Simulator`], [`PlayerAgent`]) is plain, synchronous
//! bookkeeping over a [`turnbase::Game`]: it decides whose turn it is, asks a
//! bot for an action or waits on a human, and applies the result. It has no
//! terminal or rendering code in its execution path and runs the same way in
//! a `cargo test` process as it does behind a UI.
//!
//! The `ui` feature (on by default) adds [`SimulationRunner`], a fixed
//! dashboard driven by `retroglyph-core`'s `App`/`Terminal` loop. It binds
//! straight to `retroglyph` primitives (`Terminal`, `Rect`, `print`) rather
//! than introducing a layout engine or widget abstraction of its own; a game
//! opts in by implementing [`PrintableGame`] and drawing into the rect it is
//! handed.

mod simulator;
#[cfg(feature = "ui")]
mod ui;

pub use simulator::{PlayerAgent, Simulator};
#[cfg(feature = "ui")]
pub use ui::{PrintableGame, SimulationRunner, run};
