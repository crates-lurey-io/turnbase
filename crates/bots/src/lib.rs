//! Bots for the Turnbase engine: `Random`, minimax/alpha-beta, and MCTS.
//!
//! Every bot drives a game through the [`turnbase::Game`] trait, so the same
//! bot works for any game implemented against the engine.

use turnbase::{Game, PlayerId};

mod ismcts;
mod mcts;
mod minimax;
mod random;

pub use ismcts::Ismcts;
pub use mcts::Mcts;
pub use minimax::Minimax;
pub use random::Random;

/// A policy that picks one action for a player at a decision point.
pub trait Bot<G: Game> {
    /// Returns the action to play for `player` in `state`, or `None` if there
    /// is nothing to do (no legal actions, e.g. a terminal state).
    fn choose(&mut self, game: &G, state: &G::State, player: PlayerId) -> Option<G::Action>;
}

/// A bot that scores and ranks every available action, best first.
///
/// For hints, teaching, and debugging search. An opt-in extension to [`Bot`]:
/// bots with no meaningful ranking (e.g. a uniform-random bot) simply do not
/// implement it.
pub trait RankedBot<G: Game> {
    /// Returns each legal action for `player` paired with its score, sorted
    /// best (highest score) first. Empty when there are no legal actions.
    fn rank(&mut self, game: &G, state: &G::State, player: PlayerId) -> Vec<(G::Action, f64)>;
}
