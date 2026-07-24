use std::process::ExitCode;

use blackjack::Blackjack;

fn main() -> ExitCode {
    let game = Blackjack::default();
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
