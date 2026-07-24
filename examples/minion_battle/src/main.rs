use std::process::ExitCode;

use minion_battle::MinionBattle;

fn main() -> ExitCode {
    let game = MinionBattle;
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
