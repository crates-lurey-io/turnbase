//! Play "count to 10" against a `RandomBot` in a real terminal.
//!
//! You are seat 1 (`O`... well, there's no board, just a running total):
//! Up/Down pick an amount to add, Enter commits it, Escape quits early. The
//! AI seat moves automatically about once a second. The match ends itself
//! once the total reaches 10.
//!
//! Run with `cargo run -p turnbase-simulator --example interactive`.

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::time::Duration;

use support::{CountToTen, random_seed};
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, Simulator};

fn main() -> std::io::Result<()> {
    let mut agents: HashMap<PlayerId, PlayerAgent<CountToTen>> = HashMap::new();
    agents.insert(
        PlayerId::new(0),
        PlayerAgent::Ai(Box::new(RandomBot::new(random_seed()))),
    );
    agents.insert(PlayerId::new(1), PlayerAgent::Human);

    let simulator = Simulator::new(CountToTen, random_seed(), agents);
    turnbase_simulator::run(simulator, Duration::from_millis(750))
}
