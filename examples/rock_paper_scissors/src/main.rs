use std::process::ExitCode;

use rock_paper_scissors::RockPaperScissors;

fn main() -> ExitCode {
    turnbase_cli::run(RockPaperScissors)
}
