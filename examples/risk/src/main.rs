use std::process::ExitCode;

use risk::Risk;

/// Three seats: enough for the eliminate-and-inherit rules to matter without a
/// long random-play match.
const SEATS: u8 = 3;

fn main() -> ExitCode {
    let game = Risk::new(SEATS);
    #[cfg(feature = "ui")]
    let outcome = turnbase_cli::run_tui(game);
    #[cfg(not(feature = "ui"))]
    let outcome = turnbase_cli::run(game);
    outcome
}
