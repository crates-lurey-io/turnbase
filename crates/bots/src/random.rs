//! Uniform-random legal-move bot.

use turnbase::{Game, PlayerId, Prng};

use crate::Bot;

/// Picks uniformly at random among the legal actions.
///
/// Deterministic given its seed: the same seed and the same sequence of
/// positions produce the same choices, which keeps games reproducible.
pub struct Random {
    rng: Prng,
}

impl Random {
    /// Creates a bot seeded from `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            rng: Prng::new(seed),
        }
    }
}

impl<G: Game> Bot<G> for Random {
    fn choose(&mut self, game: &G, state: &G::State, player: PlayerId) -> Option<G::Action> {
        let mut actions = game.legal_actions(state, player);
        if actions.is_empty() {
            return None;
        }
        // `below` returns a value strictly less than `len` (a `usize`), so the
        // cast back cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let index = self.rng.below(actions.len() as u64) as usize;
        Some(actions.swap_remove(index))
    }
}
