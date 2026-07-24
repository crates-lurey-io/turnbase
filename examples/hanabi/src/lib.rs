//! Hanabi (2 players): a cooperative card game whose visibility rule is the
//! exact inverse of the engine's default.
//!
//! `ARCHITECTURE.md`'s "When the default redaction rule is backwards: Hanabi"
//! is implemented literally here: [`Game::view`] is overridden (not delegated
//! to `State::view_for`) so a seated viewer sees every *other* seat's hand in
//! full but only the hinted attributes of their own. A `None` spectator sees
//! every hand. Getting your own hand right requires storing all hands
//! unconditionally and redacting in `view`, rather than the usual
//! public/private split.
//!
//! Three other things this game exercises:
//! - **Explicit chance nodes.** A card drawn to replace one played or
//!   discarded goes through [`PlayerId::CHANCE`], not an implicit roll inside
//!   `apply`, per "Randomness, part 2" in `ARCHITECTURE.md`: it's an outcome
//!   the recipient (and every other seat, since hands are visible to
//!   teammates) observes and reasons about afterward.
//! - **Cooperative reward.** [`Game::reward`] returns the same scalar (the
//!   team's score) to every seat, regardless of `player`. There is no
//!   adversary; both seats share one outcome.
//! - **`Determinize`.** An observer's own hand and the unseen deck are
//!   resampled together from what they cannot see, honoring any color/rank
//!   hints already given when a matching card remains in the unseen pool
//!   (falling back to an arbitrary unseen card otherwise -- see
//!   [`Hanabi::determinize`]'s doc for that simplification).
//!
//! Scoring is the strict tournament rule: running out of fuse tokens ends the
//! match with a score of zero, even if fireworks were already underway. The
//! reward is the raw score (sum of firework tops, not normalized).

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Determinize, Game, Pile, PlayerId, Prng};

#[cfg(feature = "ui")]
mod ui;

/// Two players share one hand size in this configuration.
const HAND_SIZE: usize = 5;

/// Hint (information) tokens start and cap at this many.
const MAX_HINTS: u8 = 8;

/// Fuse (mistake) tokens the team starts with; hitting zero ends the match.
const MAX_FUSES: u8 = 3;

/// The highest rank a firework can reach.
const MAX_RANK: u8 = 5;

/// A card: a color index (`0..num_colors`) and a rank `1..=5`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Card {
    /// The card's color, as an index into the match's configured palette.
    pub color: u8,
    /// The card's rank, `1..=5`.
    pub rank: u8,
}

/// A card in a hand, plus what its holder has been told about it.
///
/// The holder cannot see `card` itself in their own view (that's the whole
/// point); they see only whichever of these flags is set, and if set, the
/// corresponding attribute of `card`. Teammates and spectators see `card`
/// directly and ignore these flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct HeldCard {
    card: Card,
    known_color: bool,
    known_rank: bool,
}

/// A decision. `Deal` is the chance outcome for a replacement draw; the rest
/// belong to the seated player whose turn it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// Play the card at this index in your own hand.
    Play(usize),
    /// Discard the card at this index in your own hand, regaining a hint
    /// token (capped at 8).
    Discard(usize),
    /// Tell `target` seat which of their cards are this color.
    HintColor(u32, u8),
    /// Tell `target` seat which of their cards are this rank.
    HintRank(u32, u8),
    /// Chance outcome: deal this card to the seat awaiting a replacement.
    Deal(Card),
}

/// A Hanabi position for two seats.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HanabiState {
    hands: Vec<Vec<HeldCard>>,
    fireworks: Vec<u8>,
    hint_tokens: u8,
    fuse_tokens: u8,
    discard: Pile<Card>,
    deck: Pile<Card>,
    current: u32,
    /// Set right after a Play/Discard while the actor's replacement card is
    /// pending; [`PlayerId::CHANCE`] is the only active player meanwhile.
    awaiting_draw: Option<u32>,
    /// `None` until the deck runs out; then the number of further player
    /// turns (one per seat) remaining before the match ends.
    final_turns_left: Option<u32>,
    rng: Prng,
}

impl HanabiState {
    /// `seat`'s hand, in hand order (index-addressable by `Action`).
    #[must_use]
    fn hand(&self, seat: usize) -> &[HeldCard] {
        &self.hands[seat]
    }

    /// Each color's top played rank (0 if nothing has been played yet).
    #[must_use]
    pub fn fireworks(&self) -> &[u8] {
        &self.fireworks
    }

    /// Remaining hint tokens.
    #[must_use]
    pub const fn hint_tokens(&self) -> u8 {
        self.hint_tokens
    }

    /// Remaining fuse tokens; the match ends the instant this hits zero.
    #[must_use]
    pub const fn fuse_tokens(&self) -> u8 {
        self.fuse_tokens
    }

    /// The discard pile.
    #[must_use]
    pub fn discard(&self) -> &[Card] {
        self.discard.as_slice()
    }

    /// Cards remaining in the face-down deck.
    #[must_use]
    pub const fn deck_size(&self) -> usize {
        self.deck.len()
    }

    /// The seat whose turn it is (meaningless while chance is dealing).
    #[must_use]
    pub const fn current(&self) -> u32 {
        self.current
    }

    /// The team's current score: the sum of every firework's top rank.
    #[must_use]
    pub fn score(&self) -> u32 {
        self.fireworks.iter().map(|&rank| u32::from(rank)).sum()
    }
}

/// The rules of Hanabi for a chosen palette size.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Hanabi {
    num_colors: u8,
}

impl Hanabi {
    /// Creates a match using `num_colors` colors (standard Hanabi uses 5).
    #[must_use]
    pub const fn new(num_colors: u8) -> Self {
        Self { num_colors }
    }

    /// The maximum possible score: five per color.
    #[must_use]
    fn max_score(self) -> u32 {
        u32::from(self.num_colors) * u32::from(MAX_RANK)
    }
}

impl Default for Hanabi {
    fn default() -> Self {
        Self::new(5)
    }
}

/// Builds one copy of every card: rank 1 x3, ranks 2-4 x2, rank 5 x1, per
/// color. The standard Hanabi distribution.
fn full_deck(num_colors: u8) -> Vec<Card> {
    let mut cards = Vec::new();
    for color in 0..num_colors {
        for rank in 1..=MAX_RANK {
            let count = if rank == 1 {
                3
            } else if rank == MAX_RANK {
                1
            } else {
                2
            };
            for _ in 0..count {
                cards.push(Card { color, rank });
            }
        }
    }
    cards
}

/// What `viewer` sees of one card: the real card for a teammate or spectator,
/// or only the hinted attributes for the holder's own hand.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VisibleCard {
    /// A teammate's (or, for a spectator, anyone's) card, seen in full.
    Full(Card),
    /// One of the viewer's own cards: only what they have been told.
    Own {
        /// The color, if a color hint named it.
        known_color: Option<u8>,
        /// The rank, if a rank hint named it.
        known_rank: Option<u8>,
    },
}

/// What `viewer` observes: public match state plus every hand, redacted per
/// the inverted rule (see the module docs).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HanabiView {
    /// Each color's top played rank.
    pub fireworks: Vec<u8>,
    /// Remaining hint tokens.
    pub hint_tokens: u8,
    /// Remaining fuse tokens.
    pub fuse_tokens: u8,
    /// The discard pile, in discard order.
    pub discard: Vec<Card>,
    /// Cards remaining in the deck (count only; order is never observable).
    pub deck_size: usize,
    /// The seat to move.
    pub current: u32,
    /// Every seat's hand, indexed by seat: full for teammates and
    /// spectators, hint-only for the viewer's own seat.
    pub hands: Vec<Vec<VisibleCard>>,
    /// Whether the match has ended.
    pub over: bool,
}

fn seat_of(player: PlayerId) -> usize {
    usize::try_from(player.index()).expect("seat index fits usize")
}

impl Game for Hanabi {
    type State = HanabiState;
    type Action = Action;
    type View = HanabiView;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let mut rng = Prng::new(seed);
        let mut deck_items = full_deck(self.num_colors);
        rng.shuffle(&mut deck_items);
        let mut deck = Pile::from_items(deck_items);

        let mut hands: Vec<Vec<HeldCard>> = (0..2).map(|_| Vec::with_capacity(HAND_SIZE)).collect();
        for hand in &mut hands {
            for _ in 0..HAND_SIZE {
                let card = deck.draw().expect("deck holds enough cards for the deal");
                hand.push(HeldCard {
                    card,
                    known_color: false,
                    known_rank: false,
                });
            }
        }

        HanabiState {
            hands,
            fireworks: vec![0; self.num_colors as usize],
            hint_tokens: MAX_HINTS,
            fuse_tokens: MAX_FUSES,
            discard: Pile::new(),
            deck,
            current: 0,
            awaiting_draw: None,
            final_turns_left: None,
            rng,
        }
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else if state.awaiting_draw.is_some() {
            ActivePlayers::one(PlayerId::CHANCE)
        } else {
            ActivePlayers::one(PlayerId::new(state.current))
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if player.is_chance() {
            return if state.awaiting_draw.is_some() {
                state.deck.iter().map(|&card| Action::Deal(card)).collect()
            } else {
                Vec::new()
            };
        }
        if self.is_terminal(state)
            || state.awaiting_draw.is_some()
            || player.index() != state.current
        {
            return Vec::new();
        }

        let seat = seat_of(player);
        let hand_len = state.hand(seat).len();
        let mut actions: Vec<Action> = (0..hand_len).map(Action::Play).collect();
        if state.hint_tokens < MAX_HINTS {
            actions.extend((0..hand_len).map(Action::Discard));
        }
        if state.hint_tokens > 0 {
            actions.extend(hint_actions(state, seat, self.num_colors));
        }
        actions
    }

    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        if player.is_chance() {
            apply_deal(state, action);
            return;
        }
        let seat = seat_of(player);
        match action {
            Action::Play(index) => self.apply_play(state, seat, index),
            Action::Discard(index) => self.apply_discard(state, seat, index),
            Action::HintColor(target, color) => {
                self.apply_hint(state, seat, usize::try_from(target).unwrap(), |hc| {
                    if hc.card.color == color {
                        hc.known_color = true;
                    }
                });
            }
            Action::HintRank(target, rank) => {
                self.apply_hint(state, seat, usize::try_from(target).unwrap(), |hc| {
                    if hc.card.rank == rank {
                        hc.known_rank = true;
                    }
                });
            }
            Action::Deal(_) => {} // Deal is a chance-only outcome.
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.fuse_tokens == 0
            || state.score() == self.max_score()
            || state.final_turns_left == Some(0)
    }

    fn reward(&self, state: &Self::State, _player: PlayerId) -> f64 {
        // Cooperative: every seat gets the same scalar, the team's score.
        // Running the fuses out zeros the score outright (strict rule),
        // rather than crediting whatever fireworks happened to be underway.
        if state.fuse_tokens == 0 {
            0.0
        } else {
            f64::from(state.score())
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        let hands = state
            .hands
            .iter()
            .enumerate()
            .map(|(seat, hand)| {
                let is_own = viewer.is_some_and(|v| seat_of(v) == seat);
                hand.iter()
                    .map(|hc| {
                        if is_own {
                            VisibleCard::Own {
                                known_color: hc.known_color.then_some(hc.card.color),
                                known_rank: hc.known_rank.then_some(hc.card.rank),
                            }
                        } else {
                            VisibleCard::Full(hc.card)
                        }
                    })
                    .collect()
            })
            .collect();

        HanabiView {
            fireworks: state.fireworks.clone(),
            hint_tokens: state.hint_tokens,
            fuse_tokens: state.fuse_tokens,
            discard: state.discard.as_slice().to_vec(),
            deck_size: state.deck.len(),
            current: state.current,
            hands,
            over: self.is_terminal(state),
        }
    }
}

/// Legal hints from `seat`: one per color/rank actually present in the other
/// seat's hand (truthful and non-empty, per the rules).
fn hint_actions(state: &HanabiState, seat: usize, num_colors: u8) -> Vec<Action> {
    let target = other_seat(seat);
    let target_hand = state.hand(target);
    let target_u32 = u32::try_from(target).unwrap();

    let mut colors_present = vec![false; num_colors as usize];
    let mut ranks_present = [false; (MAX_RANK + 1) as usize];
    for hc in target_hand {
        colors_present[hc.card.color as usize] = true;
        ranks_present[hc.card.rank as usize] = true;
    }

    let mut actions = Vec::new();
    for (color, &present) in colors_present.iter().enumerate() {
        if present {
            actions.push(Action::HintColor(target_u32, u8::try_from(color).unwrap()));
        }
    }
    for rank in 1..=MAX_RANK {
        if ranks_present[rank as usize] {
            actions.push(Action::HintRank(target_u32, rank));
        }
    }
    actions
}

/// The other of the two seats.
const fn other_seat(seat: usize) -> usize {
    1 - seat
}

impl Hanabi {
    fn apply_play(self, state: &mut HanabiState, seat: usize, index: usize) {
        let card = state.hands[seat].remove(index).card;
        let color = usize::from(card.color);
        if card.rank == state.fireworks[color] + 1 {
            state.fireworks[color] = card.rank;
            if card.rank == MAX_RANK {
                state.hint_tokens = (state.hint_tokens + 1).min(MAX_HINTS);
            }
        } else {
            state.fuse_tokens = state.fuse_tokens.saturating_sub(1);
            state.discard.put(card);
        }
        self.finish_turn(state, seat);
    }

    fn apply_discard(self, state: &mut HanabiState, seat: usize, index: usize) {
        let card = state.hands[seat].remove(index).card;
        state.discard.put(card);
        state.hint_tokens = (state.hint_tokens + 1).min(MAX_HINTS);
        self.finish_turn(state, seat);
    }

    /// Spends a hint token and applies `mark` to every card in `target`'s
    /// hand (setting the flag the hint reveals); ends `seat`'s turn.
    fn apply_hint(
        self,
        state: &mut HanabiState,
        seat: usize,
        target: usize,
        mark: impl Fn(&mut HeldCard),
    ) {
        state.hint_tokens -= 1;
        for hc in &mut state.hands[target] {
            mark(hc);
        }
        self.finish_turn(state, seat);
    }

    /// Ends `seat`'s turn: draws a replacement via chance if the deck still
    /// has cards, otherwise advances directly (consuming one final-round
    /// turn once the deck has run out). Does nothing once the match is
    /// already over, so a fuse-out or a completed board stops immediately.
    fn finish_turn(self, state: &mut HanabiState, seat: usize) {
        if state.fuse_tokens == 0 || state.score() == self.max_score() {
            return;
        }
        if state.deck.is_empty() {
            advance_player(state);
        } else {
            state.awaiting_draw = Some(u32::try_from(seat).unwrap());
        }
    }
}

/// Commits a chance-dealt replacement card into the waiting seat's hand.
fn apply_deal(state: &mut HanabiState, action: Action) {
    let Action::Deal(card) = action else { return };
    let Some(seat) = state.awaiting_draw.take() else {
        return;
    };
    if let Some(pos) = state.deck.position(&card) {
        state.deck.remove(pos);
    }
    state.hands[usize::try_from(seat).unwrap()].push(HeldCard {
        card,
        known_color: false,
        known_rank: false,
    });

    // Advance while `final_turns_left` is still `None`, so this
    // deck-emptying transition itself is not counted as one of the final
    // turns -- only the `num_players` turns that follow are.
    let deck_now_empty = state.deck.is_empty();
    advance_player(state);
    if deck_now_empty {
        state.final_turns_left = Some(2);
    }
}

/// Moves `current` to the other seat, and if the final round has started,
/// counts this transition against it.
fn advance_player(state: &mut HanabiState) {
    state.current = u32::try_from(other_seat(usize::try_from(state.current).unwrap())).unwrap();
    if let Some(remaining) = state.final_turns_left {
        state.final_turns_left = Some(remaining.saturating_sub(1));
    }
}

/// Removes and returns one card equal to `card` from `bag`, if present.
fn take_one(bag: &mut Vec<Card>, card: Card) -> bool {
    bag.iter().position(|&c| c == card).is_some_and(|index| {
        bag.swap_remove(index);
        true
    })
}

impl Determinize for Hanabi {
    /// Resamples `observer`'s own hand and the unseen deck order together,
    /// from the multiset of cards `observer` cannot see: the full deck, minus
    /// the discard pile, minus one card per color per rank already reflected
    /// in the fireworks (each firework rank required playing exactly one
    /// card of that rank to reach), minus every other seat's visible hand.
    ///
    /// Each of `observer`'s own cards is resampled honoring its known
    /// color/rank hints when the unseen pool still has a matching card left;
    /// if none remains (every matching card is already accounted for
    /// elsewhere in the resample), it falls back to an arbitrary unseen
    /// card instead -- a documented simplification rather than a fully
    /// hint-consistent resample.
    fn determinize(&self, state: &HanabiState, observer: PlayerId, rng: &mut Prng) -> HanabiState {
        let obs = seat_of(observer);
        let mut next = state.clone();

        let mut bag = full_deck(self.num_colors);
        for &card in &next.discard {
            take_one(&mut bag, card);
        }
        for (color, &top) in next.fireworks.iter().enumerate() {
            for rank in 1..=top {
                take_one(
                    &mut bag,
                    Card {
                        color: u8::try_from(color).unwrap(),
                        rank,
                    },
                );
            }
        }
        for (seat, hand) in next.hands.iter().enumerate() {
            if seat != obs {
                for hc in hand {
                    take_one(&mut bag, hc.card);
                }
            }
        }

        rng.shuffle(&mut bag);

        // Resample the most-constrained slots first: an original card the
        // observer fully or partially knows is still (untouched) present
        // somewhere in `bag`, but an earlier *unconstrained* draw could
        // otherwise grab it arbitrarily before a later constrained slot gets
        // a chance to claim its match. Ties keep the original hand order.
        let own_len = next.hands[obs].len();
        let mut order: Vec<usize> = (0..own_len).collect();
        order.sort_by_key(|&i| {
            let hc = &next.hands[obs][i];
            std::cmp::Reverse(u8::from(hc.known_color) + u8::from(hc.known_rank))
        });

        let mut resampled: Vec<Option<HeldCard>> = vec![None; own_len];
        for i in order {
            let hc = next.hands[obs][i];
            let index = bag
                .iter()
                .position(|c| {
                    (!hc.known_color || c.color == hc.card.color)
                        && (!hc.known_rank || c.rank == hc.card.rank)
                })
                .unwrap_or(0);
            let card = bag.remove(index);
            resampled[i] = Some(HeldCard {
                card,
                known_color: hc.known_color,
                known_rank: hc.known_rank,
            });
        }
        next.hands[obs] = resampled.into_iter().map(Option::unwrap).collect();
        next.deck = Pile::from_items(bag);
        next
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // reward() is an exact integer score cast to f64
mod tests {
    use super::{Action, Card, Determinize, Game, Hanabi, HanabiState, Prng};
    use turnbase::PlayerId;

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Deals every pending chance draw so the state is ready for the next
    /// player decision; a no-op if chance is not active.
    fn resolve_chance(game: Hanabi, state: &mut HanabiState, rng: &mut Prng) {
        while game.active_players(state).contains(PlayerId::CHANCE) {
            let actions = game.legal_actions(state, PlayerId::CHANCE);
            let index = usize::try_from(rng.below(actions.len() as u64)).unwrap();
            game.apply(state, PlayerId::CHANCE, actions[index]);
        }
    }

    /// Total cards across every zone, by (color, rank); conserved always.
    fn card_counts(state: &HanabiState) -> Vec<((u8, u8), u32)> {
        let mut counts = std::collections::BTreeMap::new();
        let mut add = |card: Card| *counts.entry((card.color, card.rank)).or_insert(0u32) += 1;
        for hand in &state.hands {
            for hc in hand {
                add(hc.card);
            }
        }
        for &card in &state.discard {
            add(card);
        }
        for &card in &state.deck {
            add(card);
        }
        // Fireworks: one instance per color per played rank is "used up" and
        // not otherwise tracked anywhere else, so count it back in.
        for (color, &top) in state.fireworks.iter().enumerate() {
            for rank in 1..=top {
                add(Card {
                    color: u8::try_from(color).unwrap(),
                    rank,
                });
            }
        }
        counts.into_iter().collect()
    }

    fn full_deck_counts(game: Hanabi) -> Vec<((u8, u8), u32)> {
        let mut counts = std::collections::BTreeMap::new();
        for card in super::full_deck(game.num_colors) {
            *counts.entry((card.color, card.rank)).or_insert(0u32) += 1;
        }
        counts.into_iter().collect()
    }

    /// Plays a full match by always taking the first legal action (resolving
    /// chance automatically), returning the terminal state. Used to check
    /// termination and card conservation without needing real strategy.
    fn play_out(game: Hanabi, seed: u64) -> HanabiState {
        let mut state = game.new_initial_state(seed);
        let mut rng = Prng::new(seed ^ 0xF00D);
        let mut steps = 0;
        loop {
            resolve_chance(game, &mut state, &mut rng);
            if game.is_terminal(&state) {
                break;
            }
            let player = game.active_players(&state).iter().next().unwrap();
            let actions = game.legal_actions(&state, player);
            let index = usize::try_from(rng.below(actions.len() as u64)).unwrap();
            game.apply(&mut state, player, actions[index]);
            steps += 1;
            assert!(steps < 5_000, "seed {seed} did not terminate");
        }
        state
    }

    #[test]
    fn own_hand_is_hint_only_but_teammates_hand_is_full() {
        let game = Hanabi::default();
        let state = game.new_initial_state(1);

        let mine = game.view(&state, Some(P0));
        assert!(
            mine.hands[0]
                .iter()
                .all(|c| matches!(c, super::VisibleCard::Own { .. })),
            "own hand must be hint-only"
        );
        assert!(
            mine.hands[1]
                .iter()
                .all(|c| matches!(c, super::VisibleCard::Full(_))),
            "teammate's hand must be shown in full"
        );
    }

    #[test]
    fn spectator_sees_every_hand_in_full() {
        let game = Hanabi::default();
        let state = game.new_initial_state(1);
        let view = game.view(&state, None);
        for hand in &view.hands {
            assert!(
                hand.iter()
                    .all(|c| matches!(c, super::VisibleCard::Full(_)))
            );
        }
    }

    #[test]
    fn hints_must_be_truthful_and_nonempty() {
        let game = Hanabi::default();
        let state = game.new_initial_state(1);
        let actions = game.legal_actions(&state, P0);

        for action in &actions {
            match *action {
                Action::HintColor(target, color) => {
                    assert_eq!(target, 1);
                    assert!(
                        state.hand(1).iter().any(|hc| hc.card.color == color),
                        "hint named a color p1 does not hold"
                    );
                }
                Action::HintRank(target, rank) => {
                    assert_eq!(target, 1);
                    assert!(
                        state.hand(1).iter().any(|hc| hc.card.rank == rank),
                        "hint named a rank p1 does not hold"
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn hints_are_illegal_without_tokens() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hint_tokens = 0;
        let actions = game.legal_actions(&state, P0);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::HintColor(..) | Action::HintRank(..))),
            "no hints should be legal with zero hint tokens"
        );
    }

    #[test]
    fn hinting_yourself_is_never_offered() {
        let game = Hanabi::default();
        let state = game.new_initial_state(1);
        let actions = game.legal_actions(&state, P0);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::HintColor(0, _) | Action::HintRank(0, _))),
            "a hint must never target the actor's own seat"
        );
    }

    #[test]
    fn correct_play_advances_the_firework() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        // Rig p0's first card to be the color-0 rank-1 card.
        state.hands[0][0].card = Card { color: 0, rank: 1 };
        game.apply(&mut state, P0, Action::Play(0));
        assert_eq!(state.fireworks[0], 1);
        assert_eq!(state.fuse_tokens, 3);
    }

    #[test]
    fn wrong_play_burns_a_fuse_and_discards() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hands[0][0].card = Card { color: 0, rank: 2 }; // needs rank 1 first
        game.apply(&mut state, P0, Action::Play(0));
        assert_eq!(state.fuse_tokens, 2);
        assert_eq!(state.discard().len(), 1);
        assert_eq!(state.fireworks[0], 0);
    }

    #[test]
    fn completing_a_five_regains_a_hint_token() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hint_tokens = 7;
        state.fireworks[0] = 4;
        state.hands[0][0].card = Card { color: 0, rank: 5 };
        game.apply(&mut state, P0, Action::Play(0));
        assert_eq!(state.fireworks[0], 5);
        assert_eq!(state.hint_tokens, 8);
    }

    #[test]
    fn hint_token_never_exceeds_the_cap_via_five_or_discard() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hint_tokens = 8;
        state.fireworks[0] = 4;
        state.hands[0][0].card = Card { color: 0, rank: 5 };
        game.apply(&mut state, P0, Action::Play(0));
        assert_eq!(state.hint_tokens, 8, "capped at MAX_HINTS");
    }

    #[test]
    fn discard_regains_a_hint_token_capped_at_eight() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hint_tokens = 6;
        game.apply(&mut state, P0, Action::Discard(0));
        assert_eq!(state.hint_tokens, 7);
    }

    #[test]
    fn discard_is_illegal_when_hints_are_full() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.hint_tokens = 8;
        let actions = game.legal_actions(&state, P0);
        assert!(!actions.iter().any(|a| matches!(a, Action::Discard(_))));
    }

    #[test]
    fn score_and_reward_are_equal_across_seats_cooperative() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.fireworks = vec![2, 1, 0, 0, 0];
        assert_eq!(game.reward(&state, P0), game.reward(&state, P1));
        assert_eq!(game.reward(&state, P0), 3.0);
    }

    #[test]
    fn fuses_exhausted_zeros_the_score_for_every_seat() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        state.fireworks = vec![3, 2, 0, 0, 0];
        state.fuse_tokens = 0;
        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, P0), 0.0);
        assert_eq!(game.reward(&state, P1), 0.0);
    }

    #[test]
    fn draw_after_play_goes_through_chance() {
        let game = Hanabi::default();
        let mut state = game.new_initial_state(1);
        game.apply(&mut state, P0, Action::Discard(0));
        assert!(game.active_players(&state).contains(PlayerId::CHANCE));
        assert_eq!(state.hand(0).len(), 4);

        let mut rng = Prng::new(9);
        resolve_chance(game, &mut state, &mut rng);
        assert_eq!(state.hand(0).len(), 5);
        assert!(!game.active_players(&state).contains(PlayerId::CHANCE));
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn cards_are_conserved_through_a_full_random_match() {
        for seed in 0..15 {
            let game = Hanabi::default();
            let expected = full_deck_counts(game);
            let state = play_out(game, seed);
            assert!(game.is_terminal(&state));
            assert_eq!(card_counts(&state), expected, "seed {seed}");
        }
    }

    #[test]
    fn random_self_play_always_terminates() {
        for seed in 0..15 {
            let state = play_out(Hanabi::default(), seed);
            assert!(Hanabi::default().is_terminal(&state));
        }
    }

    #[test]
    fn same_seed_deals_the_same_hands() {
        let game = Hanabi::default();
        let a = game.new_initial_state(42);
        let b = game.new_initial_state(42);
        assert_eq!(a, b);
    }

    #[test]
    fn determinize_preserves_the_observers_view_and_the_multiset() {
        let game = Hanabi::default();
        let expected = full_deck_counts(game);
        for seed in 0..10 {
            let mut state = game.new_initial_state(seed);
            let mut walk = Prng::new(seed ^ 0xABCD);
            let mut resample = Prng::new(seed ^ 0x1234);
            let mut steps = 0;
            loop {
                resolve_chance(game, &mut state, &mut walk);
                if game.is_terminal(&state) {
                    break;
                }
                for &observer in &[P0, P1] {
                    let world = game.determinize(&state, observer, &mut resample);
                    assert_eq!(
                        game.view(&world, Some(observer)),
                        game.view(&state, Some(observer)),
                        "determinization changed observer's own view (seed {seed})"
                    );
                    assert_eq!(
                        card_counts(&world),
                        expected,
                        "determinization did not conserve cards (seed {seed})"
                    );
                }
                let player = game.active_players(&state).iter().next().unwrap();
                let actions = game.legal_actions(&state, player);
                let index = usize::try_from(walk.below(actions.len() as u64)).unwrap();
                game.apply(&mut state, player, actions[index]);
                steps += 1;
                assert!(steps < 5_000, "seed {seed} did not terminate");
            }
        }
    }
}
