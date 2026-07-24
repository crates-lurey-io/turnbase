//! Tier 1: the same `Game` plus a `PrintableGame` impl (see `ui.rs`) upgrades
//! the text `play` to the retroglyph dashboard. `turnbase_cli::run_tui` still
//! provides headless `new`/`query`/`act` and bot `self-play` unchanged.

use std::process::ExitCode;

use coup::Coup;

/// Four seats, so the challenge/block response windows Coup is built around
/// actually come into play.
const SEATS: u8 = 4;

fn main() -> ExitCode {
    turnbase_cli::run_tui(Coup::new(SEATS))
}
