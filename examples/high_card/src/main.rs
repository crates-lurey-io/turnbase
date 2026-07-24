//! High card is pure chance (deal two cards, higher wins), so `self-play` and
//! `new` deal the hands and reveal the outcome; there are no player decisions
//! to prompt. The session's chance resolution advances the deals headlessly.

use std::process::ExitCode;

use high_card::HighCard;

fn main() -> ExitCode {
    turnbase_cli::run(HighCard::default())
}
