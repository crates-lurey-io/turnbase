use std::process::ExitCode;

use risk::Risk;

/// Three seats: enough for the eliminate-and-inherit rules to matter without a
/// long random-play match.
const SEATS: u8 = 3;

fn main() -> ExitCode {
    turnbase_cli::run(Risk::new(SEATS))
}
