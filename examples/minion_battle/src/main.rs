use std::process::ExitCode;

use minion_battle::MinionBattle;

fn main() -> ExitCode {
    turnbase_cli::run(MinionBattle)
}
