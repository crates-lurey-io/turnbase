//! Play `Coup` (reused from the `examples` crate) against 1-3 AI opponents
//! in a real terminal.
//!
//! You are always seat 0. The dashboard renders from your seat's
//! `Game::View` for the whole match (see `Simulator::primary_human`), so no
//! opponent's hand ever shows up on screen — only their coins, revealed
//! (lost) cards, and remaining influence count, same as a human opponent
//! across a real table would see. Escape quits early.
//!
//! ```text
//! cargo run -p turnbase-simulator --example interactive_coup -- [OPTIONS]
//!
//! --players N         Total seats, 2-4 (default: 2). You are seat 0; the
//!                      rest are AI.
//! --difficulty LEVEL   easy | medium | hard (default: medium). See
//!                      `support::Difficulty` for what each level actually
//!                      does differently.
//! ```

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::time::Duration;

use support::{DemoCoup, Difficulty, random_seed};
use turnbase::PlayerId;
use turnbase_simulator::{PlayerAgent, Simulator};

const MIN_PLAYERS: u8 = 2;
const MAX_PLAYERS: u8 = 4;
const DEFAULT_PLAYERS: u8 = 2;
const DEFAULT_DIFFICULTY: Difficulty = Difficulty::Medium;

fn main() -> std::io::Result<()> {
    let (players, difficulty) = match parse_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}\n");
            print_usage();
            std::process::exit(2);
        }
    };

    let mut agents: HashMap<PlayerId, PlayerAgent<DemoCoup>> = HashMap::new();
    agents.insert(PlayerId::new(0), PlayerAgent::Human);
    for seat in 1..u32::from(players) {
        agents.insert(
            PlayerId::new(seat),
            PlayerAgent::Ai(difficulty.bot(random_seed())),
        );
    }

    println!("Coup: {players} players, seat 0 is you, AI difficulty {difficulty:?}. Starting...");
    let simulator = Simulator::new(
        DemoCoup(examples::Coup::new(players)),
        random_seed(),
        agents,
    );
    turnbase_simulator::run(simulator, Duration::from_millis(750))
}

/// Parses `--players N` and `--difficulty LEVEL`, in any order, both
/// optional. Returns a human-readable error for anything malformed rather
/// than panicking, since this reads real command-line input.
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(u8, Difficulty), String> {
    let mut players = DEFAULT_PLAYERS;
    let mut difficulty = DEFAULT_DIFFICULTY;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--players" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--players needs a value".to_owned())?;
                let parsed: u8 = value
                    .parse()
                    .map_err(|_| format!("'{value}' is not a number"))?;
                if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&parsed) {
                    return Err(format!(
                        "--players must be {MIN_PLAYERS}-{MAX_PLAYERS}, got {parsed}"
                    ));
                }
                players = parsed;
            }
            "--difficulty" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--difficulty needs a value".to_owned())?;
                difficulty = Difficulty::parse(&value)
                    .ok_or_else(|| format!("'{value}' is not easy, medium, or hard"))?;
            }
            other => return Err(format!("unrecognized option '{other}'")),
        }
    }

    Ok((players, difficulty))
}

fn print_usage() {
    eprintln!(
        "usage: interactive_coup [--players 2-4] [--difficulty easy|medium|hard]\n\
         defaults: --players {DEFAULT_PLAYERS} --difficulty medium"
    );
}
