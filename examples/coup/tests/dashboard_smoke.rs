//! Drives the Coup dashboard against retroglyph's headless backend for a full
//! match, proving the [`coup::Coup`] `PrintableGame` impl survives real `App`
//! frames (layout, wrapping, per-seat view rendering) without panicking. No
//! real terminal is involved; the backend renders into an in-memory grid.
//!
//! Only meaningful with the `ui` feature (which provides the dashboard), so
//! the whole file compiles away without it.
#![cfg(feature = "ui")]

use std::collections::HashMap;
use std::time::Duration;

use coup::Coup;
use retroglyph_core::backend::Headless;
use retroglyph_core::{Flow, Frame, Terminal, step};
use turnbase::PlayerId;
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, SimulationRunner, Simulator};

#[test]
fn dashboard_renders_a_full_match_headless() {
    let mut agents: HashMap<PlayerId, PlayerAgent<Coup>> = HashMap::new();
    for seat in 0..4 {
        agents.insert(
            PlayerId::new(seat),
            PlayerAgent::Ai(Box::new(RandomBot::new(u64::from(seat) + 1))),
        );
    }

    // ai_tick ZERO advances a bot seat every frame, so the match plays out in a
    // few dozen frames; the budget is generous.
    let simulator = Simulator::new(Coup::new(4), 7, agents);
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
        "an all-bot Coup match should reach a terminal state within the frame budget"
    );
}
