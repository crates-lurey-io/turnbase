//! Player identity.

/// Identifies one seat in a match.
///
/// A seat is not necessarily a human: scripted opponents, a blackjack dealer,
/// and the reserved chance "player" ([`PlayerId::CHANCE`]) are all seats. Real
/// players are numbered from 0; games map their own seat concepts onto these
/// indices.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct PlayerId(u32);

impl PlayerId {
    /// The reserved chance pseudo-player.
    ///
    /// Committed random outcomes (a card revealed, a hand dealt) are modeled as
    /// actions taken by this seat, so they become real, undoable state rather
    /// than an RNG side effect. See the chance-node design in `ARCHITECTURE.md`.
    /// Real players must not use this index.
    pub const CHANCE: Self = Self(u32::MAX);

    /// Creates a seat with the given index.
    ///
    /// Indices should be small and dense (0, 1, 2, ...); `u32::MAX` is reserved
    /// for [`PlayerId::CHANCE`].
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the zero-based seat index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }

    /// Returns true if this is the reserved chance pseudo-player.
    #[must_use]
    pub const fn is_chance(self) -> bool {
        self.0 == Self::CHANCE.0
    }
}

impl core::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_chance() {
            f.write_str("chance")
        } else {
            write!(f, "p{}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerId;

    #[test]
    fn index_round_trips() {
        assert_eq!(PlayerId::new(3).index(), 3);
    }

    #[test]
    fn chance_is_distinct_and_flagged() {
        assert!(PlayerId::CHANCE.is_chance());
        assert!(!PlayerId::new(0).is_chance());
        assert_ne!(PlayerId::CHANCE, PlayerId::new(0));
    }

    #[test]
    fn ordering_is_by_index() {
        assert!(PlayerId::new(0) < PlayerId::new(1));
        assert!(PlayerId::new(2) < PlayerId::CHANCE);
    }

    #[test]
    fn display() {
        assert_eq!(PlayerId::new(1).to_string(), "p1");
        assert_eq!(PlayerId::CHANCE.to_string(), "chance");
    }
}
