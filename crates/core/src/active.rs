//! The set of players who owe a decision right now.

use std::collections::BTreeSet;

use crate::player::PlayerId;

/// An ordered, deterministic set of players who owe a decision.
///
/// Wraps a [`BTreeSet`] so iteration order is stable (ascending by seat index)
/// rather than the per-process random order of a hashed set. Determinism is the
/// engine's core promise, so ordering is part of the observable contract: two
/// replays of the same seed and inputs must visit active players in the same
/// order. The backing collection is intentionally not exposed.
///
/// Cardinality carries meaning:
/// - empty during engine-only resolution steps (adjudicating simultaneous
///   orders, running a deterministic resolution pass),
/// - one for strictly alternating games (the common case),
/// - many during simultaneous or secret phases.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct ActivePlayers(BTreeSet<PlayerId>);

impl ActivePlayers {
    /// Returns the empty set (no player owes a decision).
    #[must_use]
    pub const fn none() -> Self {
        Self(BTreeSet::new())
    }

    /// Returns a singleton set containing just `player`.
    #[must_use]
    pub fn one(player: PlayerId) -> Self {
        let mut set = BTreeSet::new();
        set.insert(player);
        Self(set)
    }

    /// Returns the set of all real seats `0..num_players`.
    ///
    /// Does not include [`PlayerId::CHANCE`].
    #[must_use]
    pub fn all(num_players: u32) -> Self {
        (0..num_players).map(PlayerId::new).collect()
    }

    /// Returns true if `player` owes a decision.
    #[must_use]
    pub fn contains(&self, player: PlayerId) -> bool {
        self.0.contains(&player)
    }

    /// Returns the number of active players.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if no player owes a decision.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates the active players in ascending seat order.
    pub fn iter(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.0.iter().copied()
    }
}

impl FromIterator<PlayerId> for ActivePlayers {
    fn from_iter<I: IntoIterator<Item = PlayerId>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a ActivePlayers {
    type Item = PlayerId;
    type IntoIter = std::iter::Copied<std::collections::btree_set::Iter<'a, PlayerId>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::ActivePlayers;
    use crate::player::PlayerId;

    #[test]
    fn none_is_empty() {
        let a = ActivePlayers::none();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
    }

    #[test]
    fn one_contains_only_that_player() {
        let a = ActivePlayers::one(PlayerId::new(2));
        assert_eq!(a.len(), 1);
        assert!(a.contains(PlayerId::new(2)));
        assert!(!a.contains(PlayerId::new(0)));
    }

    #[test]
    fn all_covers_the_range_excluding_chance() {
        let a = ActivePlayers::all(3);
        assert_eq!(a.len(), 3);
        assert!(a.contains(PlayerId::new(0)));
        assert!(a.contains(PlayerId::new(2)));
        assert!(!a.contains(PlayerId::CHANCE));
    }

    #[test]
    fn iteration_is_ascending_and_deduplicated() {
        let a: ActivePlayers = [PlayerId::new(2), PlayerId::new(0), PlayerId::new(2)]
            .into_iter()
            .collect();
        let order: Vec<u32> = a.iter().map(PlayerId::index).collect();
        assert_eq!(order, vec![0, 2]);
    }
}
