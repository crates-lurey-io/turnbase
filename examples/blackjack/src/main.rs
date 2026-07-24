use std::process::ExitCode;

use blackjack::Blackjack;

fn main() -> ExitCode {
    let game = Blackjack::default();
    // With `ui`, `play` opens Blackjack's own bespoke retroglyph TUI (see
    // `blackjack::tui`) via the CLI's custom-play hook; the headless commands
    // stay shared. Without `ui`, `play` falls back to the text stepper.
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_with_play(game, blackjack::tui::play);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
