//! Provided state shape for games with hidden information.

use std::collections::BTreeMap;

use crate::{PlayerId, Prng};

/// State split into a public zone and per-player private zones, plus the
/// match's random generator.
///
/// Redaction is mechanical: a player's observation is the public zone plus
/// *their own* private entry, so there is no field to forget to strip. The
/// backing map is not exposed; games reach private data through the accessors.
/// Using this type is a convenience, not a requirement (`Game::State` is an
/// associated type and can be any shape) but it makes the common hidden-info
/// game turnkey.
///
/// The [`Prng`] lives here so it clones, serializes, and rewinds together with
/// everything else: snapshot and resume are O(1), and a `Reversible` undo
/// record can restore the stream position (see `ARCHITECTURE.md`). Games with
/// their own `State` type should embed a [`Prng`] the same way.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State<P, Q> {
    public: P,
    private: BTreeMap<PlayerId, Q>,
    prng: Prng,
}

impl<P, Q> State<P, Q> {
    /// Creates a state with the given public zone, no private entries, and a
    /// generator seeded from `seed`.
    #[must_use]
    pub const fn new(public: P, seed: u64) -> Self {
        Self {
            public,
            private: BTreeMap::new(),
            prng: Prng::new(seed),
        }
    }

    /// Returns the public zone, visible to everyone.
    #[must_use]
    pub const fn public(&self) -> &P {
        &self.public
    }

    /// Returns the public zone for mutation inside `apply`.
    pub const fn public_mut(&mut self) -> &mut P {
        &mut self.public
    }

    /// Sets `player`'s private zone, returning the previous value if any.
    pub fn insert_private(&mut self, player: PlayerId, value: Q) -> Option<Q> {
        self.private.insert(player, value)
    }

    /// Returns `player`'s private zone, or `None` if they have none.
    #[must_use]
    pub fn private(&self, player: PlayerId) -> Option<&Q> {
        self.private.get(&player)
    }

    /// Returns a mutable reference to `player`'s private zone.
    pub fn private_mut(&mut self, player: PlayerId) -> Option<&mut Q> {
        self.private.get_mut(&player)
    }

    /// Removes and returns `player`'s private zone, if any.
    ///
    /// Needed to reverse a deal exactly: undoing a chance move that dealt a
    /// private card must leave no stale entry behind, so a `Reversible::undo`
    /// calls this to drop the card it handed out.
    ///
    /// # Example
    /// ```
    /// use turnbase::{PlayerId, State};
    /// let mut state: State<(), u8> = State::new((), 0);
    /// state.insert_private(PlayerId::new(0), 7); // deal
    /// assert_eq!(state.remove_private(PlayerId::new(0)), Some(7)); // undo the deal
    /// assert_eq!(state.private(PlayerId::new(0)), None);
    /// ```
    pub fn remove_private(&mut self, player: PlayerId) -> Option<Q> {
        self.private.remove(&player)
    }

    /// Returns the match's generator.
    #[must_use]
    pub const fn rng(&self) -> &Prng {
        &self.prng
    }

    /// Returns the match's generator for drawing randomness inside `apply`.
    pub const fn rng_mut(&mut self) -> &mut Prng {
        &mut self.prng
    }
}

impl<P: Clone, Q: Clone> State<P, Q> {
    /// Produces the standard observation for `viewer`: the public zone plus
    /// their own private zone (`None` viewer, a spectator, sees public only).
    ///
    /// This is the default visibility rule. Games whose rule is inverted or
    /// otherwise non-standard (e.g. Hanabi) build their [`PlayerView`] directly
    /// instead of calling this. Clones the observed data, so games with a large
    /// public zone may prefer a cheaper custom projection.
    #[must_use]
    pub fn view_for(&self, viewer: Option<PlayerId>) -> PlayerView<P, Q> {
        PlayerView {
            public: self.public.clone(),
            own_private: viewer.and_then(|p| self.private(p)).cloned(),
        }
    }
}

/// The standard observation produced by [`State::view_for`]: the public zone
/// and the viewer's own private zone, if any.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerView<P, Q> {
    /// The public zone, visible to everyone.
    pub public: P,
    /// The viewer's own private zone, or `None` for a spectator.
    pub own_private: Option<Q>,
}

#[cfg(test)]
mod tests {
    use super::State;
    use crate::PlayerId;

    #[test]
    fn private_zones_are_per_player() {
        let mut state: State<u32, &str> = State::new(0, 1);
        state.insert_private(PlayerId::new(0), "a");
        state.insert_private(PlayerId::new(1), "b");
        assert_eq!(state.private(PlayerId::new(0)), Some(&"a"));
        assert_eq!(state.private(PlayerId::new(1)), Some(&"b"));
        assert_eq!(state.private(PlayerId::new(2)), None);
    }

    #[test]
    fn view_for_shows_only_the_viewers_private_zone() {
        let mut state: State<u32, &str> = State::new(7, 1);
        state.insert_private(PlayerId::new(0), "secret0");
        state.insert_private(PlayerId::new(1), "secret1");

        let view = state.view_for(Some(PlayerId::new(0)));
        assert_eq!(view.public, 7);
        assert_eq!(view.own_private, Some("secret0"));

        let spectator = state.view_for(None);
        assert_eq!(spectator.public, 7);
        assert_eq!(spectator.own_private, None);
    }
}
