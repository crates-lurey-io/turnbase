//! High card: the smallest game with committed chance and hidden information.
//!
//! Chance deals one face-down card to each of two seats from a shared deck; the
//! higher card wins. It exercises the [`PlayerId::CHANCE`] pseudo-player,
//! `chance_outcomes`, [`turnbase::sample_chance`], and a [`Reversible`] chance
//! move whose undo restores deck order exactly (so a replayed deal matches).

use std::cmp::Ordering;

use turnbase::{ActivePlayers, Game, PlayerId, PlayerView, Reversible, State};

/// Rules for a high-card match over a `deck_size`-card deck.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HighCard {
    deck_size: u8,
}

impl HighCard {
    /// Creates a match whose deck holds cards `0..deck_size`.
    #[must_use]
    pub const fn new(deck_size: u8) -> Self {
        Self { deck_size }
    }
}

/// Deals `action`'s card from the deck to the next seat, returning how to undo.
fn deal(state: &mut HighCardState, action: Action) -> Undo {
    let Action::Deal(card) = action;
    let index = state
        .public()
        .deck
        .iter()
        .position(|&c| c == card)
        .expect("a dealt card must be in the deck");
    let recipient = PlayerId::new(u32::from(state.public().dealt));
    state.public_mut().deck.remove(index);
    state.insert_private(recipient, card);
    state.public_mut().dealt += 1;
    Undo {
        index,
        card,
        recipient,
    }
}

impl Default for HighCard {
    fn default() -> Self {
        Self::new(6)
    }
}

/// Public table state: the remaining deck and how many cards have been dealt.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    deck: Vec<u8>,
    dealt: u8,
}

impl Table {
    /// The cards still in the deck, in the order chance draws consider them.
    #[must_use]
    pub const fn deck(&self) -> &[u8] {
        self.deck.as_slice()
    }

    /// How many cards have been dealt (0, 1, or 2).
    #[must_use]
    pub const fn dealt(&self) -> u8 {
        self.dealt
    }
}

/// Dealing one card from the deck to the next seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Deal the given card value.
    Deal(u8),
}

/// Reverses one deal: put the card back where it was and forget it.
pub struct Undo {
    index: usize,
    card: u8,
    recipient: PlayerId,
}

/// Public table plus each seat's face-down card.
pub type HighCardState = State<Table, u8>;

impl Game for HighCard {
    type State = HighCardState;
    type Action = Action;
    type View = PlayerView<Table, u8>;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let table = Table {
            deck: (0..self.deck_size).collect(),
            dealt: 0,
        };
        State::new(table, seed)
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::CHANCE)
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if player.is_chance() && !self.is_terminal(state) {
            state
                .public()
                .deck
                .iter()
                .map(|&c| Action::Deal(c))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        let _ = deal(state, action);
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.public().dealt >= 2
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let card0 = state.private(PlayerId::new(0)).copied();
        let card1 = state.private(PlayerId::new(1)).copied();
        let (Some(card0), Some(card1)) = (card0, card1) else {
            return 0.0;
        };
        let (mine, theirs) = if player.index() == 0 {
            (card0, card1)
        } else {
            (card1, card0)
        };
        match mine.cmp(&theirs) {
            Ordering::Greater => 1.0,
            Ordering::Less => -1.0,
            Ordering::Equal => 0.0,
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        state.view_for(viewer)
    }
}

impl Reversible for HighCard {
    type UndoRecord = Undo;

    fn apply_undoable(
        &self,
        state: &mut Self::State,
        _player: PlayerId,
        action: Self::Action,
    ) -> Self::UndoRecord {
        deal(state, action)
    }

    fn undo(&self, state: &mut Self::State, record: Self::UndoRecord) {
        state.public_mut().dealt -= 1;
        state.public_mut().deck.insert(record.index, record.card);
        state.remove_private(record.recipient);
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, HighCard};
    use proptest::prelude::*;
    use turnbase::{Game, PlayerId, Prng, Reversible, sample_chance};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Deals a full game using `seed`, returning the terminal state.
    fn deal_out(game: HighCard, seed: u64) -> <HighCard as Game>::State {
        let mut sampler = Prng::new(seed);
        let mut state = game.new_initial_state(0);
        while !game.is_terminal(&state) {
            let action = sample_chance(&game, &state, &mut sampler).unwrap();
            game.apply(&mut state, PlayerId::CHANCE, action);
        }
        state
    }

    #[test]
    fn chance_is_active_until_both_cards_are_dealt() {
        let game = HighCard::default();
        let mut state = game.new_initial_state(0);
        assert!(game.active_players(&state).contains(PlayerId::CHANCE));
        game.apply(&mut state, PlayerId::CHANCE, Action::Deal(3));
        assert!(game.active_players(&state).contains(PlayerId::CHANCE));
        game.apply(&mut state, PlayerId::CHANCE, Action::Deal(1));
        assert!(game.is_terminal(&state));
        assert!(game.active_players(&state).is_empty());
    }

    #[test]
    fn chance_outcomes_are_uniform_over_the_deck() {
        let game = HighCard::new(4);
        let state = game.new_initial_state(0);
        let outcomes = game.chance_outcomes(&state);
        assert_eq!(outcomes.len(), 4);
        for (_, probability) in &outcomes {
            assert!((probability - 0.25).abs() < 1e-12);
        }
        let sum: f64 = outcomes.iter().map(|(_, p)| p).sum();
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0; cards are distinct
    fn higher_card_wins() {
        let game = HighCard::default();
        let state = deal_out(game, 3);
        let card0 = state.private(P0).copied().unwrap();
        let card1 = state.private(P1).copied().unwrap();
        let expected = if card0 > card1 { 1.0 } else { -1.0 };
        assert_eq!(game.reward(&state, P0), expected);
        assert_eq!(game.reward(&state, P1), -expected);
    }

    #[test]
    fn a_player_sees_only_their_own_card() {
        let game = HighCard::default();
        let mut sampler = Prng::new(1);
        let mut state = game.new_initial_state(0);
        let action = sample_chance(&game, &state, &mut sampler).unwrap();
        game.apply(&mut state, PlayerId::CHANCE, action); // deals to seat 0

        assert!(game.view(&state, Some(P0)).own_private.is_some());
        assert!(game.view(&state, Some(P1)).own_private.is_none());
        assert!(game.view(&state, None).own_private.is_none());
    }

    #[test]
    fn deals_are_reproducible() {
        let game = HighCard::default();
        let a = deal_out(game, 7);
        let b = deal_out(game, 7);
        assert_eq!(a, b);
    }

    proptest! {
        /// Undoing a deal restores the deck order, count, and private zones
        /// exactly, so a re-drawn card lands identically.
        #[test]
        fn undo_restores_state(seed in any::<u64>()) {
            let game = HighCard::default();
            let mut sampler = Prng::new(seed);
            let mut state = game.new_initial_state(0);
            while !game.is_terminal(&state) {
                let action = sample_chance(&game, &state, &mut sampler).unwrap();
                let before = state.clone();
                let record = game.apply_undoable(&mut state, PlayerId::CHANCE, action);
                prop_assert_ne!(&state, &before);
                game.undo(&mut state, record);
                prop_assert_eq!(&state, &before);
                game.apply(&mut state, PlayerId::CHANCE, action);
            }
        }
    }
}
