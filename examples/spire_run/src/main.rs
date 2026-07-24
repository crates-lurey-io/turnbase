use std::process::ExitCode;

use spire_run::SpireRun;

fn main() -> ExitCode {
    let game = SpireRun;
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
