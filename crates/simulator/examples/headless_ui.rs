//! Exercises `SimulationRunner`/`PrintableGame` against `Headless`, without a
//! real terminal, for every demo game. Not a demo to run for output (the
//! backend renders into an in-memory grid nobody prints) so much as a
//! compile-and-smoke-test that each game's `PrintableGame` impl survives a
//! few real `App` frames.
//!
//! See `interactive.rs`, `interactive_minion_battle.rs`, and
//! `interactive_coup.rs` for versions of these same games that actually open
//! a terminal.

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use retroglyph_core::backend::Headless;
use retroglyph_core::{Flow, Frame, Terminal, step};
use support::{CountToTen, DemoCoup, DemoMinionBattle};
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, PrintableGame, SimulationRunner, Simulator};

/// Drives an all-AI match for a handful of frames against `Headless`,
/// printing how far it got. `frames` is chosen generously enough (each
/// game's own turn/step cap, or comfortably past its typical random-play
/// length) that the match reaches a terminal state and this also exercises
/// the "match over" pause described in `SimulationRunner`'s docs.
fn smoke_test<G: PrintableGame + Default>(name: &str, frames: u64)
where
    G::Action: Debug,
{
    let mut agents: HashMap<PlayerId, PlayerAgent<G>> = HashMap::new();
    agents.insert(
        PlayerId::new(0),
        PlayerAgent::Ai(Box::new(RandomBot::new(1))),
    );
    agents.insert(
        PlayerId::new(1),
        PlayerAgent::Ai(Box::new(RandomBot::new(2))),
    );

    let simulator = Simulator::new(G::default(), 0, agents);
    let mut runner = SimulationRunner::new(simulator, Duration::ZERO);
    let mut term = Terminal::new(Headless::new(60, 24));

    let mut frames_run = 0u64;
    for frame in 0..frames {
        let ctx = Frame {
            delta: Duration::from_millis(16),
            frame,
        };
        frames_run += 1;
        if step(&mut term, &mut runner, &ctx) == Flow::Exit {
            break;
        }
    }

    let sim = runner.into_simulator();
    println!(
        "{name}: drove {frames_run} frame(s), terminal={}, log entries={}",
        sim.is_terminal(),
        sim.log_history().len()
    );
}

fn main() {
    smoke_test::<CountToTen>("CountToTen", 20);
    // MinionBattle::TURN_CAP is 100 (one apply == one turn), so 110 frames
    // guarantees a terminal state regardless of random draws.
    smoke_test::<DemoMinionBattle>("MinionBattle", 110);
    smoke_test::<DemoCoup>("Coup", 200);
}
