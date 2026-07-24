use std::process::ExitCode;

use woodland::Woodland;

fn main() -> ExitCode {
    let game = Woodland;
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
