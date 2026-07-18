//! Play `MinionBattle` (reused from the `examples` crate) against a
//! `RandomBot` in a real terminal.
//!
//! You are seat 1. Each turn is one action: attack with a minion (against
//! the enemy hero or one of its minions) or end your turn — every action
//! ends the turn, so play strictly alternates. Watch the log panel for
//! deathrattle cascades when a minion dies. Escape quits early.
//!
//! Run with `cargo run -p turnbase-simulator --example interactive_minion_battle`.

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::time::Duration;

use support::DemoMinionBattle;
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, Simulator};

fn main() -> std::io::Result<()> {
    let mut agents: HashMap<PlayerId, PlayerAgent<DemoMinionBattle>> = HashMap::new();
    agents.insert(
        PlayerId::new(0),
        PlayerAgent::Ai(Box::new(RandomBot::new(1))),
    );
    agents.insert(PlayerId::new(1), PlayerAgent::Human);

    let simulator = Simulator::new(DemoMinionBattle::default(), 0, agents);
    turnbase_simulator::run(simulator, Duration::from_millis(750))
}
