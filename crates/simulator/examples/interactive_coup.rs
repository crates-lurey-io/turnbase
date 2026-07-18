//! Play `Coup` (reused from the `examples` crate) against a `RandomBot` in a
//! real terminal.
//!
//! You are seat 1. The dashboard renders from your seat's `Game::View` for
//! the whole match (see `Simulator::primary_human`), so the AI's hand never
//! shows up on screen — only its coins, revealed (lost) cards, and remaining
//! influence count, same as a human opponent across a real table would see.
//! Escape quits early.
//!
//! Run with `cargo run -p turnbase-simulator --example interactive_coup`.

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::time::Duration;

use support::DemoCoup;
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, Simulator};

fn main() -> std::io::Result<()> {
    let mut agents: HashMap<PlayerId, PlayerAgent<DemoCoup>> = HashMap::new();
    agents.insert(
        PlayerId::new(0),
        PlayerAgent::Ai(Box::new(RandomBot::new(1))),
    );
    agents.insert(PlayerId::new(1), PlayerAgent::Human);

    let simulator = Simulator::new(DemoCoup::default(), 0, agents);
    turnbase_simulator::run(simulator, Duration::from_millis(750))
}
