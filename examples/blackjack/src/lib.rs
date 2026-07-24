//! Blackjack: dealer vs. one player over a single hand, dealt from a shared
//! shoe.
//!
//! Pressure-tests four corners of the engine at once:
//!
//! - **Scripted participant.** Seat 1 (the dealer) is an ordinary entry in
//!   `active_players`; its `legal_actions` always returns exactly one
//!   rule-computed action (hit under 17, stand otherwise), per the "Scripted /
//!   automated participants" pattern in `ARCHITECTURE.md`. No bot, no special
//!   "NPC" concept.
//! - **Committed chance nodes.** Every card, from the opening deal through
//!   every hit, is dealt by [`PlayerId::CHANCE`] drawing from the shoe, never
//!   an implicit roll inside `apply`.
//! - **Hidden information.** The dealer's hole card lives in seat 1's private
//!   zone (via [`State`]'s default redaction) until the player stands or
//!   busts, at which point it is moved into the public zone.
//! - **A dense `step_reward` hook.** [`Game::reward`] remains the only true
//!   terminal signal (+1 win / -1 loss / 0 push, and the negation for the
//!   dealer); `step_reward` additionally reports -1.0 for whichever seat's
//!   most recent card busted them, purely to demonstrate the RL dense-signal
//!   hook.
//!
//! [`Determinize`] resamples the dealer's hidden hole card and the unseen
//! shoe order for the player observer, so `Ismcts` can search this game.

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Determinize, Game, Pile, PlayerId, Prng, State};

#[cfg(feature = "ui")]
mod ui;

/// Seat 0: the human (or bot) player.
pub const PLAYER: PlayerId = PlayerId::new(0);
/// Seat 1: the scripted dealer.
pub const DEALER: PlayerId = PlayerId::new(1);

/// A single playing card, one of 13 ranks (suits do not affect blackjack
/// value, so the shoe carries four indistinguishable copies of each rank).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Card(u8);

impl Card {
    /// Creates a card of the given `rank` (1 = ace, 11-13 = jack/queen/king).
    ///
    /// # Panics
    /// Panics if `rank` is not in `1..=13`.
    #[must_use]
    pub const fn new(rank: u8) -> Self {
        assert!(rank >= 1 && rank <= 13, "rank must be 1..=13");
        Self(rank)
    }

    /// Returns true if this card is an ace.
    #[must_use]
    pub const fn is_ace(self) -> bool {
        self.0 == 1
    }

    /// Returns this card's blackjack value: an ace counts high (11) here,
    /// [`best_total`] softens it to 1 as needed to avoid busting.
    #[must_use]
    pub const fn value(self) -> u8 {
        if self.0 == 1 {
            11
        } else if self.0 >= 10 {
            10
        } else {
            self.0
        }
    }
}

impl std::fmt::Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            1 => "A",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            9 => "9",
            10 => "10",
            11 => "J",
            12 => "Q",
            _ => "K",
        })
    }
}

/// Returns the best (highest, non-busting if possible) total for `hand`,
/// counting aces as 11 and softening them to 1 one at a time while the total
/// would otherwise bust.
#[must_use]
pub fn best_total(hand: &[Card]) -> u8 {
    let mut total: i32 = hand.iter().map(|c| i32::from(c.value())).sum();
    let mut soft_aces = hand.iter().filter(|c| c.is_ace()).count();
    while total > 21 && soft_aces > 0 {
        total -= 10;
        soft_aces -= 1;
    }
    // The opening hand (two cards) plus any number of subsequent hits can
    // reach at most 11 * 11 + 10 * 9 or so before every ace is softened; the
    // shoe (52 cards) bounds the hand length well inside u8 range.
    u8::try_from(total.max(0)).unwrap_or(0)
}

/// Returns true if `hand`'s best total still counts an ace as 11 (a "soft"
/// total that cannot bust from a single more point of value).
#[must_use]
pub fn is_soft(hand: &[Card]) -> bool {
    let mut total: i32 = hand.iter().map(|c| i32::from(c.value())).sum();
    let mut soft_aces = hand.iter().filter(|c| c.is_ace()).count();
    while total > 21 && soft_aces > 0 {
        total -= 10;
        soft_aces -= 1;
    }
    soft_aces > 0
}

/// One decision point in a hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// Take another card.
    Hit,
    /// Take no more cards.
    Stand,
    /// A committed chance outcome: deal `Card` to whichever seat is currently
    /// awaiting one.
    Deal(Card),
}

/// Which phase of the hand is in progress, and so who is active.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    /// The opening deal: player, dealer up-card, player, dealer hole card
    /// (`0..=3`, the count of opening cards dealt so far).
    Opening(u8),
    /// The player is deciding whether to hit or stand.
    PlayerTurn,
    /// The player hit; chance owes them one card.
    PlayerDraw,
    /// The dealer's scripted turn (hole card already revealed).
    DealerTurn,
    /// The dealer hit; chance owes it one card.
    DealerDraw,
    /// The hand is over.
    Done,
}

/// The hand's final result, from the player's perspective.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Outcome {
    /// The player's hand beat the dealer's (or the dealer busted).
    PlayerWin,
    /// The dealer's hand beat the player's (or the player busted).
    DealerWin,
    /// Equal totals: no one wins.
    Push,
}

/// Public table state: the shoe, both hands' face-up cards, and the phase.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Table {
    shoe: Pile<Card>,
    player_hand: Vec<Card>,
    dealer_hand: Vec<Card>,
    hole_revealed: bool,
    phase: Phase,
    outcome: Option<Outcome>,
    /// The seat whose most recently dealt card busted their hand, if any --
    /// consulted only by [`Blackjack::step_reward`] (see its docs).
    busted: Option<PlayerId>,
}

impl Table {
    /// The cards remaining in the shoe, in the order chance draws consider
    /// them.
    #[must_use]
    pub fn shoe(&self) -> &[Card] {
        self.shoe.as_slice()
    }

    /// The player's face-up cards.
    #[must_use]
    pub fn player_hand(&self) -> &[Card] {
        &self.player_hand
    }

    /// The dealer's face-up cards (the up-card, plus the hole card once
    /// revealed, plus any hits).
    #[must_use]
    pub fn dealer_hand(&self) -> &[Card] {
        &self.dealer_hand
    }

    /// True once the dealer's hole card has been revealed into the public
    /// zone (on the player standing or busting).
    #[must_use]
    pub const fn hole_revealed(&self) -> bool {
        self.hole_revealed
    }

    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The hand's result, once [`Phase::Done`].
    #[must_use]
    pub const fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// The player's current best total.
    #[must_use]
    pub fn player_total(&self) -> u8 {
        best_total(&self.player_hand)
    }

    /// The dealer's current best total over its face-up cards only (the hole
    /// card, while hidden, does not count toward this).
    #[must_use]
    pub fn dealer_total(&self) -> u8 {
        best_total(&self.dealer_hand)
    }
}

/// Full blackjack state: the public table plus the dealer's hidden hole card
/// (private to seat 1 until revealed).
pub type BlackjackState = State<Table, Card>;

/// What a viewer observes: both hands' face-up cards and the shoe's *size*,
/// never its order.
///
/// Built by hand rather than via [`State::view_for`] (which would clone the
/// whole public zone, including the shoe's exact card order) because the shoe
/// order is exactly the information [`Determinize`] resamples: exposing it in
/// the view would make every determinization visibly change what the player
/// "sees", defeating the point of resampling only what's actually hidden. Only
/// the viewer's own hole card (the dealer's, from the dealer's own viewpoint)
/// is ever `Some`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BlackjackView {
    /// The player's face-up cards.
    pub player_hand: Vec<Card>,
    /// The dealer's face-up cards.
    pub dealer_hand: Vec<Card>,
    /// True once the dealer's hole card has joined `dealer_hand`.
    pub hole_revealed: bool,
    /// How many cards remain in the shoe.
    pub shoe_size: usize,
    /// The current phase.
    pub phase: Phase,
    /// The hand's result, once [`Phase::Done`].
    pub outcome: Option<Outcome>,
    /// The viewer's own hidden hole card, if they have one right now (only
    /// ever the dealer, before it is revealed).
    pub own_hole_card: Option<Card>,
}

/// The rules of a single blackjack hand. Carries no configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Blackjack;

/// Moves `card` out of the shoe and hands it to whichever seat `Phase`
/// indicates is awaiting one, advancing the phase.
fn deal(state: &mut BlackjackState, card: Card) {
    let index = state
        .public()
        .shoe
        .position(&card)
        .expect("a dealt card must be in the shoe");
    state.public_mut().shoe.remove(index);

    match state.public().phase {
        Phase::Opening(step) => {
            match step {
                0 | 2 => state.public_mut().player_hand.push(card),
                1 => state.public_mut().dealer_hand.push(card),
                _ => {
                    state.insert_private(DEALER, card);
                }
            }
            let next_step = step + 1;
            state.public_mut().phase = if next_step >= 4 {
                Phase::PlayerTurn
            } else {
                Phase::Opening(next_step)
            };
        }
        Phase::PlayerDraw => {
            state.public_mut().player_hand.push(card);
            if best_total(&state.public().player_hand) > 21 {
                state.public_mut().busted = Some(PLAYER);
                reveal_hole(state);
                finish(state, Outcome::DealerWin);
            } else {
                state.public_mut().phase = Phase::PlayerTurn;
            }
        }
        Phase::DealerDraw => {
            state.public_mut().dealer_hand.push(card);
            if best_total(&state.public().dealer_hand) > 21 {
                state.public_mut().busted = Some(DEALER);
                finish(state, Outcome::PlayerWin);
            } else {
                state.public_mut().phase = Phase::DealerTurn;
            }
        }
        Phase::PlayerTurn | Phase::DealerTurn | Phase::Done => {
            unreachable!("chance is only active during a deal phase")
        }
    }
}

/// Moves the dealer's hidden hole card into the public hand, if it hasn't
/// been already (a no-op if the hand somehow has no hole card left, e.g.
/// called twice).
fn reveal_hole(state: &mut BlackjackState) {
    if let Some(card) = state.remove_private(DEALER) {
        state.public_mut().dealer_hand.push(card);
    }
    state.public_mut().hole_revealed = true;
}

/// Records the terminal `outcome` and ends the hand.
const fn finish(state: &mut BlackjackState, outcome: Outcome) {
    state.public_mut().outcome = Some(outcome);
    state.public_mut().phase = Phase::Done;
}

/// Compares standing totals once the dealer stands without busting.
fn settle(state: &mut BlackjackState) {
    let player = best_total(state.public().player_hand());
    let dealer = best_total(state.public().dealer_hand());
    let outcome = match player.cmp(&dealer) {
        std::cmp::Ordering::Greater => Outcome::PlayerWin,
        std::cmp::Ordering::Less => Outcome::DealerWin,
        std::cmp::Ordering::Equal => Outcome::Push,
    };
    finish(state, outcome);
}

impl Game for Blackjack {
    type State = BlackjackState;
    type Action = Action;
    type View = BlackjackView;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let mut shoe = Pile::new();
        for rank in 1..=13u8 {
            for _ in 0..4 {
                shoe.put(Card::new(rank));
            }
        }
        let table = Table {
            shoe,
            player_hand: Vec::new(),
            dealer_hand: Vec::new(),
            hole_revealed: false,
            phase: Phase::Opening(0),
            outcome: None,
            busted: None,
        };
        let mut state = State::new(table, seed);
        // Shuffle with a copy of the state's own generator, then write its
        // advanced position back: a single deterministic shoe order per seed,
        // distinct from the chance sampler that later chooses among
        // `legal_actions(CHANCE)`.
        let mut shuffler = *state.rng();
        state.public_mut().shoe.shuffle(&mut shuffler);
        state.rng_mut().set_position(shuffler.position());
        state
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        match state.public().phase {
            Phase::Opening(_) | Phase::PlayerDraw | Phase::DealerDraw => {
                ActivePlayers::one(PlayerId::CHANCE)
            }
            Phase::PlayerTurn => ActivePlayers::one(PLAYER),
            Phase::DealerTurn => ActivePlayers::one(DEALER),
            Phase::Done => ActivePlayers::none(),
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if player.is_chance() {
            return match state.public().phase {
                Phase::Opening(_) | Phase::PlayerDraw | Phase::DealerDraw => state
                    .public()
                    .shoe
                    .iter()
                    .map(|&c| Action::Deal(c))
                    .collect(),
                _ => Vec::new(),
            };
        }
        match (state.public().phase, player) {
            (Phase::PlayerTurn, p) if p == PLAYER => vec![Action::Hit, Action::Stand],
            (Phase::DealerTurn, p) if p == DEALER => {
                // Scripted: exactly one rule-computed action, per
                // ARCHITECTURE.md's "Scripted / automated participants".
                if best_total(state.public().dealer_hand()) < 17 {
                    vec![Action::Hit]
                } else {
                    vec![Action::Stand]
                }
            }
            _ => Vec::new(),
        }
    }

    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        match (action, player) {
            (Action::Deal(card), p) if p.is_chance() => deal(state, card),
            (Action::Hit, p) if p == PLAYER => state.public_mut().phase = Phase::PlayerDraw,
            (Action::Stand, p) if p == PLAYER => {
                reveal_hole(state);
                state.public_mut().phase = Phase::DealerTurn;
            }
            (Action::Hit, p) if p == DEALER => state.public_mut().phase = Phase::DealerDraw,
            (Action::Stand, p) if p == DEALER => settle(state),
            _ => {}
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        matches!(state.public().phase, Phase::Done)
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let Some(outcome) = state.public().outcome else {
            return 0.0;
        };
        let player_reward = match outcome {
            Outcome::PlayerWin => 1.0,
            Outcome::DealerWin => -1.0,
            Outcome::Push => 0.0,
        };
        if player == PLAYER {
            player_reward
        } else {
            -player_reward
        }
    }

    /// Illustrates the dense per-step RL hook: [`Self::reward`] above remains
    /// the only true terminal signal, but a bust is a natural place to also
    /// hand back an immediate penalty (rather than waiting for the terminal
    /// state to report it). Returns 0.0 on every other step.
    fn step_reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        if state.public().busted == Some(player) {
            -1.0
        } else {
            0.0
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        let own_hole_card = viewer.and_then(|p| state.private(p)).copied();
        let table = state.public();
        BlackjackView {
            player_hand: table.player_hand.clone(),
            dealer_hand: table.dealer_hand.clone(),
            hole_revealed: table.hole_revealed,
            shoe_size: table.shoe.len(),
            phase: table.phase,
            outcome: table.outcome,
            own_hole_card,
        }
    }
}

impl Determinize for Blackjack {
    /// Resamples what the player observer cannot see: the dealer's hole card
    /// (while still hidden) and the shoe's order. Everything the player can
    /// already see -- their own hand and the dealer's up-card(s) -- is left
    /// untouched, so [`Game::view`] for the player is unchanged. The dealer
    /// has no hidden information from its own perspective (it can see its own
    /// hole card), so a dealer-observer determinization is just a clone.
    fn determinize(&self, state: &Self::State, observer: PlayerId, rng: &mut Prng) -> Self::State {
        let mut next = state.clone();
        if observer != PLAYER || next.public().hole_revealed() {
            return next;
        }

        let hole = next.remove_private(DEALER);
        let mut bag: Vec<Card> = next.public().shoe.iter().copied().collect();
        if let Some(card) = hole {
            bag.push(card);
        }
        rng.shuffle(&mut bag);

        if hole.is_some() {
            // The hole card is drawn from the resampled bag so it stays
            // consistent with the (also resampled) shoe order.
            let new_hole = bag.pop().expect("bag held the hole card just pushed");
            next.insert_private(DEALER, new_hole);
        }
        next.public_mut().shoe = bag.into_iter().collect();
        next
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Blackjack, Card, DEALER, Outcome, PLAYER, Phase, best_total, is_soft};
    use turnbase::{Determinize, Game, PlayerId, Prng, sample_chance};

    /// Plays chance nodes with a fixed sampler until a real seat is active (or
    /// the hand ends), returning the resulting state.
    fn resolve_chance(game: Blackjack, state: &mut <Blackjack as Game>::State, rng: &mut Prng) {
        while game.active_players(state).contains(PlayerId::CHANCE) {
            let action = sample_chance(&game, state, rng).unwrap();
            game.apply(state, PlayerId::CHANCE, action);
        }
    }

    /// Deals the opening four cards with a fixed sampler.
    fn opening(game: Blackjack, seed: u64) -> (<Blackjack as Game>::State, Prng) {
        let mut state = game.new_initial_state(0);
        let mut sampler = Prng::new(seed);
        resolve_chance(game, &mut state, &mut sampler);
        (state, sampler)
    }

    #[test]
    fn opening_deal_goes_player_dealer_player_hole() {
        let game = Blackjack;
        let (state, _) = opening(game, 1);
        assert_eq!(state.public().player_hand().len(), 2);
        assert_eq!(state.public().dealer_hand().len(), 1, "hole card is hidden");
        assert!(game.active_players(&state).contains(PLAYER));
    }

    #[test]
    fn hole_card_is_hidden_from_player_and_spectator_but_visible_to_dealer() {
        let game = Blackjack;
        let (state, _) = opening(game, 2);
        assert!(game.view(&state, Some(PLAYER)).own_hole_card.is_none());
        assert!(game.view(&state, None).own_hole_card.is_none());
        assert!(game.view(&state, Some(DEALER)).own_hole_card.is_some());
    }

    #[test]
    fn dealer_scripted_action_is_a_singleton() {
        let game = Blackjack;
        let (mut state, mut sampler) = opening(game, 3);
        // Force to the dealer's turn regardless of what the player drew.
        game.apply(&mut state, PLAYER, Action::Stand);
        while game.active_players(&state).contains(DEALER) {
            let actions = game.legal_actions(&state, DEALER);
            assert_eq!(actions.len(), 1, "the dealer never has a real choice");
            let expected = if best_total(state.public().dealer_hand()) < 17 {
                Action::Hit
            } else {
                Action::Stand
            };
            assert_eq!(actions[0], expected);
            game.apply(&mut state, DEALER, actions[0]);
            resolve_chance(game, &mut state, &mut sampler);
        }
        assert!(game.is_terminal(&state));
    }

    #[test]
    fn dealer_stands_on_seventeen_or_more() {
        let game = Blackjack;
        let mut state = game.new_initial_state(0);
        state.public_mut().dealer_hand.push(Card::new(10));
        state.public_mut().dealer_hand.push(Card::new(7));
        state.public_mut().phase = Phase::DealerTurn;
        assert_eq!(game.legal_actions(&state, DEALER), vec![Action::Stand]);
    }

    #[test]
    fn dealer_hits_under_seventeen() {
        let game = Blackjack;
        let mut state = game.new_initial_state(0);
        state.public_mut().dealer_hand.push(Card::new(10));
        state.public_mut().dealer_hand.push(Card::new(6));
        state.public_mut().phase = Phase::DealerTurn;
        assert_eq!(game.legal_actions(&state, DEALER), vec![Action::Hit]);
    }

    #[test]
    fn player_bust_ends_the_hand_immediately() {
        let game = Blackjack;
        let mut state = game.new_initial_state(0);
        state.public_mut().player_hand.push(Card::new(10));
        state.public_mut().player_hand.push(Card::new(9));
        state.public_mut().dealer_hand.push(Card::new(5));
        state.insert_private(DEALER, Card::new(6));
        state.public_mut().phase = Phase::PlayerDraw;
        // Rig the shoe so the only possible deal busts the player.
        state.public_mut().shoe = std::iter::once(Card::new(5)).collect();

        let actions = game.legal_actions(&state, PlayerId::CHANCE);
        assert_eq!(actions, vec![Action::Deal(Card::new(5))]);
        game.apply(&mut state, PlayerId::CHANCE, actions[0]);

        assert!(game.is_terminal(&state));
        assert_eq!(state.public().outcome(), Some(Outcome::DealerWin));
        assert!(
            state.public().hole_revealed(),
            "hole card is revealed even on a player bust, so the terminal view is consistent"
        );
    }

    #[allow(clippy::float_cmp)] // reward() is exactly one of 1.0 / -1.0 / 0.0
    #[test]
    fn reward_is_win_loss_or_push_and_dealer_is_the_negation() {
        let game = Blackjack;
        let mut state = game.new_initial_state(0);

        for (player_cards, dealer_cards, expected) in [
            (vec![10, 9], vec![10, 6], Outcome::PlayerWin),
            (vec![10, 6], vec![10, 9], Outcome::DealerWin),
            (vec![10, 8], vec![10, 8], Outcome::Push),
        ] {
            state.public_mut().player_hand = player_cards.into_iter().map(Card::new).collect();
            state.public_mut().dealer_hand = dealer_cards.into_iter().map(Card::new).collect();
            state.public_mut().phase = Phase::DealerTurn;
            game.apply(&mut state, DEALER, Action::Stand);
            assert_eq!(state.public().outcome(), Some(expected));

            let (player_reward, dealer_reward) = match expected {
                Outcome::PlayerWin => (1.0, -1.0),
                Outcome::DealerWin => (-1.0, 1.0),
                Outcome::Push => (0.0, 0.0),
            };
            assert_eq!(game.reward(&state, PLAYER), player_reward);
            assert_eq!(game.reward(&state, DEALER), dealer_reward);
        }
    }

    #[allow(clippy::float_cmp)] // step_reward() is exactly -1.0 or 0.0
    #[test]
    fn step_reward_penalizes_only_the_seat_that_just_busted() {
        let game = Blackjack;
        let mut state = game.new_initial_state(0);
        state.public_mut().player_hand.push(Card::new(10));
        state.public_mut().player_hand.push(Card::new(9));
        state.public_mut().phase = Phase::PlayerDraw;
        state.public_mut().shoe = std::iter::once(Card::new(5)).collect();

        assert_eq!(game.step_reward(&state, PLAYER), 0.0, "no bust yet");
        let action = game.legal_actions(&state, PlayerId::CHANCE)[0];
        game.apply(&mut state, PlayerId::CHANCE, action);

        assert_eq!(game.step_reward(&state, PLAYER), -1.0);
        assert_eq!(game.step_reward(&state, DEALER), 0.0);
    }

    #[test]
    fn determinize_keeps_the_players_view_invariant() {
        let game = Blackjack;
        let (state, _) = opening(game, 4);
        let before = game.view(&state, Some(PLAYER));

        let mut rng = Prng::new(99);
        for _ in 0..20 {
            let sampled = game.determinize(&state, PLAYER, &mut rng);
            assert_eq!(game.view(&sampled, Some(PLAYER)), before);
            // Card counts are preserved: nothing is created or destroyed.
            assert_eq!(sampled.public().shoe().len(), state.public().shoe().len());
        }
    }

    #[test]
    fn determinize_is_a_clone_once_the_hole_card_is_revealed() {
        let game = Blackjack;
        let (mut state, _) = opening(game, 5);
        game.apply(&mut state, PLAYER, Action::Stand); // reveals the hole card
        let mut rng = Prng::new(1);
        assert_eq!(game.determinize(&state, PLAYER, &mut rng), state);
    }

    #[test]
    fn same_seed_same_result() {
        let game = Blackjack;
        let run = |seed: u64| {
            let mut state = game.new_initial_state(0);
            let mut sampler = Prng::new(seed);
            loop {
                resolve_chance(game, &mut state, &mut sampler);
                if game.is_terminal(&state) {
                    break;
                }
                let active = game.active_players(&state).iter().next().unwrap();
                // Always take the first legal action: deterministic given the
                // seed, since the dealer's is a singleton and this always picks
                // Stand for the player once available.
                let actions = game.legal_actions(&state, active);
                let action = if active == PLAYER {
                    Action::Stand
                } else {
                    actions[0]
                };
                game.apply(&mut state, active, action);
            }
            state
        };
        assert_eq!(run(42), run(42));
    }

    #[test]
    fn best_total_softens_aces_only_as_needed_to_avoid_busting() {
        let soft = [Card::new(1), Card::new(6)]; // A, 6 = soft 17
        assert_eq!(best_total(&soft), 17);
        assert!(is_soft(&soft));

        let busted_without_softening = [Card::new(1), Card::new(6), Card::new(10)]; // A, 6, 10
        assert_eq!(best_total(&busted_without_softening), 17);
        assert!(!is_soft(&busted_without_softening));

        let two_aces = [Card::new(1), Card::new(1), Card::new(9)]; // A, A, 9 = 21
        assert_eq!(best_total(&two_aces), 21);
    }
}
