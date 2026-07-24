//! Rock-paper-scissors: the smallest simultaneous, secret game.
//!
//! Both seats are active at once and each throws in secret. There is no
//! `resolve()` hook on the engine: simultaneity is a convention. Each `apply`
//! records one throw into that seat's private zone, `active_players` shrinks as
//! seats submit, and the throw that completes the pair resolves the round into
//! the public zone (revealing both throws). Submission order does not affect the
//! result.

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Game, PlayerId, PlayerView, State};

const P0: PlayerId = PlayerId::new(0);
const P1: PlayerId = PlayerId::new(1);

/// A throw. Also the action type: a seat's move is the throw it commits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Throw {
    /// Beats scissors, loses to paper.
    Rock,
    /// Beats rock, loses to scissors.
    Paper,
    /// Beats paper, loses to rock.
    Scissors,
}

impl Throw {
    const fn beats(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Rock, Self::Scissors)
                | (Self::Scissors, Self::Paper)
                | (Self::Paper, Self::Rock)
        )
    }
}

/// The revealed result of a round, written to the public zone once both seats
/// have thrown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Resolution {
    /// The throws, indexed by seat.
    pub throws: [Throw; 2],
    /// The winning seat, or `None` for a draw.
    pub winner: Option<PlayerId>,
}

/// Public zone: empty until the round resolves, then the revealed result.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Table {
    result: Option<Resolution>,
}

impl Table {
    /// The revealed result, or `None` while throws are still secret.
    #[must_use]
    pub const fn result(&self) -> Option<&Resolution> {
        self.result.as_ref()
    }
}

/// The rules. Carries no configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct RockPaperScissors;

/// Public table plus each seat's secret throw.
pub type RpsState = State<Table, Throw>;

impl Game for RockPaperScissors {
    type State = RpsState;
    type Action = Throw;
    type View = PlayerView<Table, Throw>;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        State::new(Table::default(), seed)
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if state.public().result().is_some() {
            return ActivePlayers::none();
        }
        [P0, P1]
            .into_iter()
            .filter(|&seat| state.private(seat).is_none())
            .collect()
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        let eligible = state.public().result().is_none()
            && (player == P0 || player == P1)
            && state.private(player).is_none();
        if eligible {
            vec![Throw::Rock, Throw::Paper, Throw::Scissors]
        } else {
            Vec::new()
        }
    }

    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        state.insert_private(player, action);
        // The throw that completes the pair resolves the round. Order-free:
        // it reads both private throws regardless of who submitted last.
        if let (Some(&first), Some(&second)) = (state.private(P0), state.private(P1)) {
            let winner = if first == second {
                None
            } else if first.beats(second) {
                Some(P0)
            } else {
                Some(P1)
            };
            state.public_mut().result = Some(Resolution {
                throws: [first, second],
                winner,
            });
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.public().result().is_some()
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        match state.public().result().and_then(|r| r.winner) {
            Some(winner) if winner == player => 1.0,
            Some(_) => -1.0,
            None => 0.0,
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        state.view_for(viewer)
    }
}

#[cfg(test)]
mod tests {
    use super::{RockPaperScissors, Throw};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    #[test]
    fn both_seats_are_active_until_they_throw() {
        let game = RockPaperScissors;
        let mut state = game.new_initial_state(0);
        let active = game.active_players(&state);
        assert!(active.contains(P0) && active.contains(P1));

        game.apply(&mut state, P0, Throw::Rock);
        let active = game.active_players(&state);
        assert!(!active.contains(P0) && active.contains(P1));
        assert!(
            game.legal_actions(&state, P0).is_empty(),
            "P0 already threw"
        );

        game.apply(&mut state, P1, Throw::Scissors);
        assert!(game.is_terminal(&state));
        assert!(game.active_players(&state).is_empty());
    }

    #[test]
    fn throws_stay_secret_until_both_submit() {
        let game = RockPaperScissors;
        let mut state = game.new_initial_state(0);
        game.apply(&mut state, P0, Throw::Rock);

        // P1 (and a spectator) cannot see P0's throw before resolving.
        assert!(game.view(&state, Some(P1)).public.result().is_none());
        assert!(game.view(&state, None).public.result().is_none());
        // P0 still sees their own throw via the private zone.
        assert_eq!(game.view(&state, Some(P0)).own_private, Some(Throw::Rock));
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
    fn rock_beats_scissors() {
        let game = RockPaperScissors;
        let mut state = game.new_initial_state(0);
        game.apply(&mut state, P0, Throw::Rock);
        game.apply(&mut state, P1, Throw::Scissors);
        assert_eq!(game.reward(&state, P0), 1.0);
        assert_eq!(game.reward(&state, P1), -1.0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
    fn identical_throws_draw() {
        let game = RockPaperScissors;
        let mut state = game.new_initial_state(0);
        game.apply(&mut state, P0, Throw::Paper);
        game.apply(&mut state, P1, Throw::Paper);
        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, P0), 0.0);
        assert_eq!(game.reward(&state, P1), 0.0);
    }

    #[test]
    fn submission_order_does_not_change_the_result() {
        let game = RockPaperScissors;

        let mut p0_first = game.new_initial_state(0);
        game.apply(&mut p0_first, P0, Throw::Rock);
        game.apply(&mut p0_first, P1, Throw::Paper);

        let mut p1_first = game.new_initial_state(0);
        game.apply(&mut p1_first, P1, Throw::Paper);
        game.apply(&mut p1_first, P0, Throw::Rock);

        assert_eq!(
            p0_first.public().result(),
            p1_first.public().result(),
            "the round resolves the same regardless of who submitted first"
        );
        assert_eq!(
            p0_first.public().result().unwrap().winner,
            Some(P1),
            "paper beats rock"
        );
    }
}
