//! Interactive human client for [`turnbase`]: a fixed [`retroglyph`](retroglyph_core)
//! terminal dashboard over a [`turnbase_match::Simulator`].
//!
//! The turn loop itself ([`Simulator`], [`PlayerAgent`]) lives in
//! `turnbase-match` and has no rendering in its call path; this crate adds the
//! human-facing layer. [`SimulationRunner`] binds straight to `retroglyph-core`
//! primitives (`Terminal`, `Rect`, `print`) rather than introducing a layout
//! engine or widget abstraction of its own; a game opts in by implementing
//! [`PrintableGame`] and drawing into the rect it is handed.
//!
//! [`Simulator`] and [`PlayerAgent`] are re-exported so a dashboard user builds
//! a match without depending on `turnbase-match` directly.

mod ui;

pub use turnbase_match::{PlayerAgent, Simulator};
pub use ui::{PrintableGame, SimulationRunner, run};
