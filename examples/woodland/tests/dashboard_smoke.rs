//! Drives the Woodland dashboard against retroglyph's headless backend to a
//! terminal state, proving the [`woodland::Woodland`] `PrintableGame` impl
//! survives real `App` frames (the ring, both factions' markers and VP)
//! without panicking. No real terminal is involved.
#![cfg(feature = "ui")]

use std::collections::HashMap;
use std::time::Duration;

use retroglyph_core::backend::Headless;
use retroglyph_core::{Flow, Frame, Terminal, step};
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, SimulationRunner, Simulator};
use woodland::Woodland;

#[test]
fn dashboard_renders_a_full_match_headless() {
    let mut agents: HashMap<PlayerId, PlayerAgent<Woodland>> = HashMap::new();
    agents.insert(
        PlayerId::new(0),
        PlayerAgent::Ai(Box::new(RandomBot::new(1))),
    );
    agents.insert(
        PlayerId::new(1),
        PlayerAgent::Ai(Box::new(RandomBot::new(2))),
    );

    let simulator = Simulator::new(Woodland, 0, agents);
    let mut runner = SimulationRunner::new(simulator, Duration::ZERO);
    let mut term = Terminal::new(Headless::new(80, 24));

    let mut reached_terminal = false;
    for frame in 0..2000u64 {
        let ctx = Frame {
            delta: Duration::from_millis(16),
            frame,
        };
        if step(&mut term, &mut runner, &ctx) == Flow::Exit {
            break;
        }
        if runner.is_terminal() {
            reached_terminal = true;
            break;
        }
    }

    assert!(
        reached_terminal,
        "a woodland match should reach a terminal state within the frame budget"
    );
}
