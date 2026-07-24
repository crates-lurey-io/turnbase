use std::process::ExitCode;

use hanabi::Hanabi;

fn main() -> ExitCode {
    let game = Hanabi::default();
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
