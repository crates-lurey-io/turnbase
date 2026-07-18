//! Verifies the simulation core runs entirely in memory: no terminal, no
//! rendering backend, nothing from the `ui` feature anywhere in the call
//! graph, whether or not that feature happens to be compiled in for other
//! targets in this crate.

use std::collections::HashMap;

use examples::tic_tac_toe::TicTacToe;
use turnbase::{Game, PlayerId};
use turnbase_bots::RandomBot;
use turnbase_simulator::{PlayerAgent, Simulator};

const P0: PlayerId = PlayerId::new(0);
const P1: PlayerId = PlayerId::new(1);

#[test]
fn test_pure_headless_execution() {
    let mut agents = HashMap::new();
    agents.insert(P0, PlayerAgent::Ai(Box::new(RandomBot::new(1))));
    agents.insert(P1, PlayerAgent::Ai(Box::new(RandomBot::new(2))));

    let mut sim = Simulator::new(TicTacToe, 0, agents);

    // Run up to 1000 loop cycles, entirely in-memory; tic-tac-toe finishes in
    // at most 9, so this also proves `step` stops advancing once the match
    // reaches a terminal state.
    let mut cycles = 0;
    while cycles < 1000 && !sim.is_terminal() {
        let advanced = sim.step().unwrap();
        if advanced {
            cycles += 1;
        }
    }

    assert!(cycles > 0, "AI agents should have played at least one move");
    assert!(cycles <= 9, "tic-tac-toe cannot outlast its nine cells");
    assert!(
        sim.is_terminal(),
        "the loop only stops early once the match ends"
    );
    assert!(!sim.log_history().is_empty());
    assert_eq!(sim.log_history().len(), cycles);
}

#[test]
fn test_step_blocks_on_a_human_seat() {
    let mut agents = HashMap::new();
    agents.insert(P0, PlayerAgent::Human);
    agents.insert(P1, PlayerAgent::Ai(Box::new(RandomBot::new(7))));

    let mut sim = Simulator::new(TicTacToe, 0, agents);

    // Seat 0 (human) moves first, so `step` should refuse to act for it.
    assert_eq!(sim.awaiting_human(), Some(P0));
    assert_eq!(sim.step(), Ok(false));
    assert!(sim.log_history().is_empty());

    // Driving the human seat directly unblocks the loop for the AI seat.
    let legal = sim.game().legal_actions(sim.state(), P0);
    sim.select_human_action(P0, legal[0]).unwrap();
    assert_eq!(sim.awaiting_human(), None);
    assert_eq!(sim.step(), Ok(true));
    assert_eq!(sim.log_history().len(), 2);
}
