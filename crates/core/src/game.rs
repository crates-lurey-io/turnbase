//! The core `Game` trait and its optional make/unmake extension.

use crate::{ActivePlayers, Error, PlayerId};

/// A turn-based game defined as pure functions from state and action to new
/// state.
///
/// Per-match configuration (player count, board size, variant rules) lives on
/// the implementing value (`&self`), so [`Self::State`] stays lean and cheap to
/// clone (cloning is the default backtracking primitive) and there is one home
/// for `num_players` and variant flags. Everything is synchronous and
/// side-effect-free; randomness comes from a generator stored inside the state.
pub trait Game {
    /// A full position in a match.
    type State;

    /// A single decision. `PartialEq` powers the default [`Self::is_legal`]
    /// membership check; it is trivially derivable for the small enums and
    /// indices actions usually are.
    type Action: PartialEq;

    /// What a player or spectator observes, produced by [`Self::view`].
    type View;

    /// Returns the initial position for a match, its generator seeded from
    /// `seed`.
    fn new_initial_state(&self, seed: u64) -> Self::State;

    /// Returns the number of seats in this match: the fixed roster, seats
    /// `0..num_players`, excluding the chance pseudo-player.
    ///
    /// A constant property of the configured match, not a live count and not a
    /// capacity. It does not shrink when a player is eliminated (an eliminated
    /// seat simply stops appearing in [`Self::active_players`] and has no legal
    /// actions). "Who owes a decision right now" is [`Self::active_players`];
    /// the seats valid for [`Self::reward`] and [`Self::legal_actions`] are
    /// `0..num_players`.
    fn num_players(&self) -> usize;

    /// Returns the players who owe a decision right now.
    ///
    /// Empty during engine-only resolution, one for alternating play, many
    /// during simultaneous or secret phases. May contain [`PlayerId::CHANCE`]
    /// when a committed random outcome is pending.
    fn active_players(&self, state: &Self::State) -> ActivePlayers;

    /// Returns the actions available to `player` at this decision point.
    ///
    /// A full turn is a sequence of `apply` calls until [`Self::active_players`]
    /// changes; this enumerates one decision point, not a whole turn.
    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action>;

    /// Returns whether `action` is legal for `player` right now.
    ///
    /// Defaults to membership in [`Self::legal_actions`]. Override with a direct
    /// check for decision points whose legal set is too large to enumerate
    /// (arbitrary map targeting, payment combinatorics).
    fn is_legal(&self, state: &Self::State, player: PlayerId, action: &Self::Action) -> bool {
        self.legal_actions(state, player).contains(action)
    }

    /// Advances `state` in place by applying `player`'s `action`.
    ///
    /// Assumes the action is legal for an active player; callers that want
    /// checking use [`Self::apply_cloned`]. Draw any randomness from the
    /// generator inside `state` so the effect stays reproducible.
    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action);

    /// Returns a copy of `state` advanced by `action`, without mutating the
    /// original. The default backtracking primitive for search and tests.
    ///
    /// # Errors
    /// Returns [`Error::NotActive`] if `player` is not currently active, or
    /// [`Error::IllegalAction`] if the action is not legal for them.
    fn apply_cloned(
        &self,
        state: &Self::State,
        player: PlayerId,
        action: Self::Action,
    ) -> Result<Self::State, Error>
    where
        Self::State: Clone,
    {
        if !self.active_players(state).contains(player) {
            return Err(Error::NotActive { player });
        }
        if !self.is_legal(state, player, &action) {
            return Err(Error::IllegalAction { player });
        }
        let mut next = state.clone();
        self.apply(&mut next, player, action);
        Ok(next)
    }

    /// Returns whether the match has ended.
    fn is_terminal(&self, state: &Self::State) -> bool;

    /// Returns `player`'s terminal outcome as a single scalar (the minimal
    /// win/loss signal for search and RL). Meaningful only when
    /// [`Self::is_terminal`] holds; richer scoring belongs in public state.
    fn reward(&self, state: &Self::State, player: PlayerId) -> f64;

    /// Returns `player`'s immediate reward for the last step.
    ///
    /// Defaults to 0 (all signal at the terminal). Override to give RL trainers
    /// a dense, per-step, per-player signal.
    fn step_reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let _ = (state, player);
        0.0
    }

    /// Returns what `viewer` is allowed to observe. `None` is a seatless
    /// spectator and observes the public projection only.
    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View;
}

/// Opt-in make/unmake for games where cloning the whole state per search node
/// is too slow (chess-scale branching).
///
/// Cloning ([`Game::apply_cloned`]) is the default and is always correct;
/// implement this only when per-node cloning dominates the search budget. A
/// wrong `undo` corrupts search silently instead of crashing, so it is a
/// deliberate opt-in.
///
/// RNG invariant: [`Self::UndoRecord`] must capture the generator's position as
/// it was *before* the move (a small `Copy` value, [`Prng::position`]) and
/// [`Self::undo`] must restore it. A move may consume a variable number of
/// draws, so the pre-move position cannot be recovered by counting draws; it
/// must be snapshotted up front. The clone path gets this for free by cloning
/// the whole state.
///
/// [`Prng::position`]: crate::Prng::position
pub trait Reversible: Game {
    /// Enough information to reverse one [`Self::apply_undoable`] call.
    type UndoRecord;

    /// Advances `state` in place like [`Game::apply`], returning a record that
    /// [`Self::undo`] can use to reverse it exactly (including the generator
    /// position).
    fn apply_undoable(
        &self,
        state: &mut Self::State,
        player: PlayerId,
        action: Self::Action,
    ) -> Self::UndoRecord;

    /// Reverses the move that produced `record`, restoring `state` (and its
    /// generator position) to what it was before.
    fn undo(&self, state: &mut Self::State, record: Self::UndoRecord);
}

#[cfg(test)]
mod tests {
    use super::{Game, Reversible};
    use crate::{ActivePlayers, PlayerId, State};

    /// Two-seat toy game: each turn the active player "rolls" a random 1..=6
    /// into a shared total. It embeds a `Prng` (via `State<P, Q>`) and consumes
    /// a variable number of draws per move, so it exercises `apply_cloned`, the
    /// standard view, and the `Reversible` RNG-snapshot invariant.
    struct RollGame;

    #[derive(Clone, PartialEq, Eq, Debug)]
    enum Action {
        Roll,
    }

    type RollState = State<u32, u32>;

    struct Undo {
        prev_total: u32,
        prev_position: u64,
    }

    impl Game for RollGame {
        type State = RollState;
        type Action = Action;
        type View = crate::state::PlayerView<u32, u32>;

        fn new_initial_state(&self, seed: u64) -> Self::State {
            State::new(0, seed)
        }

        fn num_players(&self) -> usize {
            2
        }

        fn active_players(&self, state: &Self::State) -> ActivePlayers {
            if self.is_terminal(state) {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::new(*state.public() % 2))
            }
        }

        fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
            if self.is_terminal(state) {
                vec![]
            } else {
                vec![Action::Roll]
            }
        }

        fn apply(&self, state: &mut Self::State, _player: PlayerId, _action: Self::Action) {
            let roll = state.rng_mut().range(1, 7);
            *state.public_mut() += u32::try_from(roll).unwrap();
        }

        fn is_terminal(&self, state: &Self::State) -> bool {
            *state.public() >= 20
        }

        fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
            // The player who crossed 20 (the previous mover) wins.
            let winner = (*state.public() + 1) % 2;
            if player.index() == winner { 1.0 } else { -1.0 }
        }

        fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
            state.view_for(viewer)
        }
    }

    impl Reversible for RollGame {
        type UndoRecord = Undo;

        fn apply_undoable(
            &self,
            state: &mut Self::State,
            player: PlayerId,
            action: Self::Action,
        ) -> Self::UndoRecord {
            let record = Undo {
                prev_total: *state.public(),
                prev_position: state.rng().position(),
            };
            self.apply(state, player, action);
            record
        }

        fn undo(&self, state: &mut Self::State, record: Self::UndoRecord) {
            *state.public_mut() = record.prev_total;
            state.rng_mut().set_position(record.prev_position);
        }
    }

    #[test]
    fn apply_cloned_rejects_out_of_turn_and_illegal() {
        let game = RollGame;
        let state = game.new_initial_state(1);
        // Seat 1 is not active on the first turn (total 0 -> seat 0).
        assert!(
            game.apply_cloned(&state, PlayerId::new(1), Action::Roll)
                .is_err()
        );
    }

    #[test]
    fn apply_cloned_does_not_touch_the_original() {
        let game = RollGame;
        let state = game.new_initial_state(1);
        let next = game
            .apply_cloned(&state, PlayerId::new(0), Action::Roll)
            .unwrap();
        assert_eq!(*state.public(), 0);
        assert!(*next.public() >= 1 && *next.public() <= 6);
    }

    #[test]
    fn reversible_round_trip_restores_state_and_rng() {
        let game = RollGame;
        let mut state = game.new_initial_state(42);
        let before = state.clone();

        let record = game.apply_undoable(&mut state, PlayerId::new(0), Action::Roll);
        assert_ne!(state, before);
        game.undo(&mut state, record);
        assert_eq!(state, before, "undo restores total and generator position");
    }

    #[test]
    fn undo_then_reapply_reproduces_the_same_draw() {
        let game = RollGame;
        let mut state = game.new_initial_state(42);

        let first = {
            let r = game.apply_undoable(&mut state, PlayerId::new(0), Action::Roll);
            let total = *state.public();
            game.undo(&mut state, r);
            total
        };
        // Because undo rewound the generator, the replay draws identically.
        game.apply(&mut state, PlayerId::new(0), Action::Roll);
        assert_eq!(*state.public(), first);
    }
}
