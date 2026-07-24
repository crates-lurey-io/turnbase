//! Tier 1: the same `Game` plus a `PrintableGame` impl (see `ui.rs`) upgrades
//! the text `play` to the retroglyph dashboard. With `--no-default-features
//! --features cli` this drops to the Tier-0 text runner; either way
//! `turnbase_cli` provides headless `new`/`query`/`act` and bot `self-play`.

use std::process::ExitCode;

use coup::Coup;

/// Four seats, so the challenge/block response windows Coup is built around
/// actually come into play.
const SEATS: u8 = 4;

fn main() -> ExitCode {
    let game = Coup::new(SEATS);
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
