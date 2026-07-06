//! Coup (2-player MVP): a bluffing game of hidden influence.
//!
//! This scaffold builds the turn machine with the two uncontested actions
//! (Income and Coup) plus the lose-an-influence decision point, hidden hands,
//! elimination, and the win condition. Character actions with challenge/block
//! response windows land on top of this in a later step (see
//! `.matan/coup-plan.md`).
//!
//! The state has three zones: public fields, each seat's face-down `hands`
//! (private), and the `deck` (hidden from everyone). `view` returns the public
//! fields plus the viewer's own hand, never the deck or the opponent's hand.

use turnbase::{ActivePlayers, Game, PlayerId, Prng};

/// A character card. The deck holds three of each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Character {
    /// Tax; blocks Foreign Aid.
    Duke,
    /// Assassinate.
    Assassin,
    /// Steal; blocks Steal.
    Captain,
    /// Exchange; blocks Steal.
    Ambassador,
    /// Blocks Assassinate.
    Contessa,
}

const CHARACTERS: [Character; 5] = [
    Character::Duke,
    Character::Assassin,
    Character::Captain,
    Character::Ambassador,
    Character::Contessa,
];

/// A move. Only the scaffold subset for now; response actions arrive later.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Take one coin. Uncontested.
    Income,
    /// Pay seven coins; the opponent loses an influence. Forced at 10+ coins.
    Coup,
    /// Reveal and discard the influence at this hand index (when forced to lose).
    Lose(usize),
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Phase {
    ChooseAction,
    Lose { who: u8 },
    GameOver,
}

/// A Coup position.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoupState {
    coins: [u8; 2],
    hands: [Vec<Character>; 2],
    lost: [Vec<Character>; 2],
    deck: Vec<Character>,
    current: u8,
    phase: Phase,
    rng: Prng,
}

impl CoupState {
    /// Coins held by seat `player`.
    #[must_use]
    pub const fn coins(&self, player: usize) -> u8 {
        self.coins[player]
    }

    /// Number of face-down influence cards seat `player` still holds.
    #[must_use]
    pub const fn influence(&self, player: usize) -> usize {
        self.hands[player].len()
    }

    /// The revealed (lost) cards of seat `player`, face up and public.
    #[must_use]
    pub fn lost(&self, player: usize) -> &[Character] {
        &self.lost[player]
    }

    /// Seat `player`'s face-down hand. For tests and the owning player's view;
    /// never shown to opponents.
    #[must_use]
    pub fn hand(&self, player: usize) -> &[Character] {
        &self.hands[player]
    }

    /// The seat whose turn it is.
    #[must_use]
    pub const fn current(&self) -> u8 {
        self.current
    }

    /// Whether the match has ended.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        matches!(self.phase, Phase::GameOver)
    }

    const fn end_turn(&mut self) {
        self.current = 1 - self.current;
        self.phase = Phase::ChooseAction;
    }
}

/// What `viewer` observes: the public fields plus their own hand.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoupView {
    /// Coins held by each seat.
    pub coins: [u8; 2],
    /// Revealed cards of each seat.
    pub lost: [Vec<Character>; 2],
    /// Face-down influence count of each seat.
    pub influence: [usize; 2],
    /// Cards remaining in the hidden deck (count only).
    pub deck_size: usize,
    /// The seat to move.
    pub current: u8,
    /// The viewer's own hand, or empty for a seatless spectator.
    pub own_hand: Vec<Character>,
    /// Whether the match is over.
    pub over: bool,
}

/// The rules of 2-player Coup.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Coup;

impl Coup {
    const fn opponent(seat: u8) -> u8 {
        1 - seat
    }
}

impl Game for Coup {
    type State = CoupState;
    type Action = Action;
    type View = CoupView;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let mut rng = Prng::new(seed);
        let mut deck = Vec::with_capacity(15);
        for character in CHARACTERS {
            for _ in 0..3 {
                deck.push(character);
            }
        }
        rng.shuffle(&mut deck);

        let mut hands: [Vec<Character>; 2] = [Vec::new(), Vec::new()];
        for hand in &mut hands {
            hand.push(deck.pop().unwrap());
            hand.push(deck.pop().unwrap());
        }

        CoupState {
            coins: [2, 2],
            hands,
            lost: [Vec::new(), Vec::new()],
            deck,
            current: 0,
            phase: Phase::ChooseAction,
            rng,
        }
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        match state.phase {
            Phase::ChooseAction => ActivePlayers::one(PlayerId::new(u32::from(state.current))),
            Phase::Lose { who } => ActivePlayers::one(PlayerId::new(u32::from(who))),
            Phase::GameOver => ActivePlayers::none(),
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        let seat = player.index();
        match state.phase {
            Phase::ChooseAction if seat == u32::from(state.current) => {
                let coins = state.coins[seat as usize];
                if coins >= 10 {
                    return vec![Action::Coup];
                }
                let mut actions = vec![Action::Income];
                if coins >= 7 {
                    actions.push(Action::Coup);
                }
                actions
            }
            Phase::Lose { who } if seat == u32::from(who) => (0..state.hands[who as usize].len())
                .map(Action::Lose)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        match action {
            Action::Income => {
                state.coins[state.current as usize] += 1;
                state.end_turn();
            }
            Action::Coup => {
                state.coins[state.current as usize] -= 7;
                let target = Self::opponent(state.current);
                state.phase = Phase::Lose { who: target };
            }
            Action::Lose(index) => {
                let Phase::Lose { who } = state.phase else {
                    return;
                };
                let seat = who as usize;
                let card = state.hands[seat].remove(index);
                state.lost[seat].push(card);
                if state.hands[seat].is_empty() {
                    state.phase = Phase::GameOver;
                } else {
                    state.end_turn();
                }
            }
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.is_over()
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let seat = player.index() as usize;
        let opponent = 1 - seat;
        match (
            state.hands[seat].is_empty(),
            state.hands[opponent].is_empty(),
        ) {
            (false, true) => 1.0,
            (true, false) => -1.0,
            _ => 0.0,
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        let own_hand = viewer
            .map(|p| state.hands[p.index() as usize].clone())
            .unwrap_or_default();
        CoupView {
            coins: state.coins,
            lost: state.lost.clone(),
            influence: [state.hands[0].len(), state.hands[1].len()],
            deck_size: state.deck.len(),
            current: state.current,
            own_hand,
            over: state.is_over(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Coup, CoupState};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    fn seat(state: &CoupState) -> PlayerId {
        PlayerId::new(u32::from(state.current()))
    }

    /// Plays incomes until it is P0's turn with at least `coins` coins.
    fn build_coins(game: Coup, seed: u64, coins: u8) -> CoupState {
        let mut state = game.new_initial_state(seed);
        while !(state.current() == 0 && state.coins(0) >= coins) {
            let mover = seat(&state);
            game.apply(&mut state, mover, Action::Income);
        }
        state
    }

    #[test]
    fn setup_deals_two_influence_and_two_coins() {
        let game = Coup;
        let state = game.new_initial_state(1);
        assert_eq!(state.influence(0), 2);
        assert_eq!(state.influence(1), 2);
        assert_eq!(state.coins(0), 2);
        assert_eq!(state.coins(1), 2);
        // 15 cards, 4 dealt.
        assert_eq!(game.view(&state, None).deck_size, 11);
    }

    #[test]
    fn income_adds_a_coin_and_passes_the_turn() {
        let game = Coup;
        let mut state = game.new_initial_state(1);
        game.apply(&mut state, P0, Action::Income);
        assert_eq!(state.coins(0), 3);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn coup_is_illegal_below_seven_coins() {
        let game = Coup;
        let state = game.new_initial_state(1);
        assert!(!game.legal_actions(&state, P0).contains(&Action::Coup));
    }

    #[test]
    fn coup_forces_the_opponent_to_lose_an_influence() {
        let game = Coup;
        let mut state = build_coins(game, 1, 7);
        game.apply(&mut state, P0, Action::Coup);
        assert_eq!(state.coins(0), 0);

        // The opponent now chooses which influence to lose.
        assert_eq!(game.active_players(&state).iter().next(), Some(P1));
        game.apply(&mut state, P1, Action::Lose(0));
        assert_eq!(state.influence(1), 1);
        assert_eq!(state.lost(1).len(), 1);
        assert_eq!(state.current(), 1, "turn passed after the coup resolved");
    }

    #[test]
    fn ten_coins_forces_a_coup() {
        let game = Coup;
        let state = build_coins(game, 2, 10);
        assert_eq!(game.legal_actions(&state, P0), vec![Action::Coup]);
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 1.0 / -1.0
    fn losing_the_last_influence_ends_the_game() {
        let game = Coup;
        let mut state = build_coins(game, 3, 7);
        // First coup: opponent drops to one influence.
        game.apply(&mut state, P0, Action::Coup);
        game.apply(&mut state, P1, Action::Lose(0));
        // Get P0 back to seven coins and coup again.
        while !(state.current() == 0 && state.coins(0) >= 7) {
            let mover = seat(&state);
            game.apply(&mut state, mover, Action::Income);
        }
        game.apply(&mut state, P0, Action::Coup);
        game.apply(&mut state, P1, Action::Lose(0));

        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, P0), 1.0);
        assert_eq!(game.reward(&state, P1), -1.0);
    }

    #[test]
    fn a_player_sees_only_their_own_hand() {
        let game = Coup;
        let state = game.new_initial_state(1);
        assert_eq!(game.view(&state, Some(P0)).own_hand, state.hand(0));
        assert_eq!(game.view(&state, Some(P1)).own_hand, state.hand(1));
        assert!(game.view(&state, None).own_hand.is_empty());
    }
}
