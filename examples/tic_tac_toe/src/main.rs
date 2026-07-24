//! Tier 0: a `Game` plus serde derives is all it takes. `turnbase_cli::run`
//! provides headless `new`/`query`/`act`, bot `self-play`, and a text `play`
//! with no rendering code and no terminal dependency.

use std::process::ExitCode;

use tic_tac_toe::TicTacToe;

fn main() -> ExitCode {
    turnbase_cli::run(TicTacToe)
}
