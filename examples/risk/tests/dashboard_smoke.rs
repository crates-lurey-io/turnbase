//! Drives the Risk dashboard against retroglyph's headless backend for a few
//! hundred frames, proving the [`risk::Risk`] `PrintableGame` impl survives real
//! `App` frames (map layout, per-seat summary) without panicking. Risk seldom
//! terminates under random play, so this asserts progress and no panic rather
//! than a terminal state.
#![cfg(feature = "ui")]

use std::collections::HashMap;
use std::time::Duration;

use retroglyph_core::backend::Headless;
use retroglyph_core::{Flow, Frame, Terminal, step};
use risk::Risk;
use turnbase::PlayerId;
use turnbase_bots::Random;
use turnbase_simulator::{PlayerAgent, SimulationRunner, Simulator};

#[test]
fn dashboard_renders_without_panicking() {
    let mut agents: HashMap<PlayerId, PlayerAgent<Risk>> = HashMap::new();
    for seat in 0..3 {
        agents.insert(
            PlayerId::new(seat),
            PlayerAgent::Ai(Box::new(Random::new(u64::from(seat) + 1))),
        );
    }

    let simulator = Simulator::new(Risk::new(3), 9, agents);
    let mut runner = SimulationRunner::new(simulator, Duration::ZERO);
    let mut term = Terminal::new(Headless::new(80, 24));

    for frame in 0..300u64 {
        let ctx = Frame {
            delta: Duration::from_millis(16),
            frame,
        };
        if step(&mut term, &mut runner, &ctx) == Flow::Exit {
            break;
        }
    }

    let sim = runner.into_simulator();
    assert!(
        !sim.log_history().is_empty(),
        "the match should have advanced over hundreds of rendered frames"
    );
}
