//! Coup (2-player MVP): a bluffing game of hidden influence.
//!
//! This is the response-window validation from `ARCHITECTURE.md`: a turn is not
//! "act, next player" but a small state machine of decision points. Declaring a
//! character action opens a `Respond` window (the opponent may pass, challenge,
//! or block); a block opens a `RespondToBlock` window (the actor may pass or
//! challenge). Every window is just a `Phase` with `active_players` and
//! `legal_actions` computed from it. Nothing bespoke; the same primitives that
//! run tic-tac-toe run the challenge/block flow.
//!
//! Three state zones: public fields, each seat's face-down `hands` (private),
//! and the `deck` (hidden from everyone). `view` returns public + own hand.
//! Exchange (Ambassador) is deferred; all five characters are still in the deck
//! (Ambassador is claimable as a Steal blocker). See `.matan/coup-plan.md`.

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
    /// Exchange (deferred); blocks Steal.
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

/// A move. The set is phase-dependent: turn actions during `ChooseAction`,
/// responses during the windows, and `Lose` when an influence must be revealed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// +1 coin. Uncontested.
    Income,
    /// +2 coins. Blockable by Duke.
    ForeignAid,
    /// Pay 7; opponent loses an influence. Forced at 10+. Uncontested.
    Coup,
    /// Claim Duke for +3 coins. Challengeable.
    Tax,
    /// Claim Assassin, pay 3; opponent loses an influence. Challengeable,
    /// blockable by Contessa.
    Assassinate,
    /// Claim Captain, take 2 coins. Challengeable, blockable by Captain or
    /// Ambassador.
    Steal,
    /// Allow the pending action or block to stand.
    Pass,
    /// Challenge the pending claim.
    Challenge,
    /// Block the pending action by claiming this character.
    Block(Character),
    /// Reveal and discard the influence at this hand index.
    Lose(usize),
}

/// The action being resolved and who can respond to it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Pending {
    action: Action,
    actor: u8,
    claim: Option<Character>,
    block_options: Vec<Character>,
}

/// What to run once a `Lose` is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resume {
    EndTurn,
    ApplyThenEnd { action: Action, actor: u8 },
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Phase {
    ChooseAction,
    Respond {
        pending: Pending,
    },
    RespondToBlock {
        pending: Pending,
        blocker: u8,
        block_as: Character,
    },
    Lose {
        who: u8,
        resume: Resume,
    },
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

    fn end_turn(&mut self) {
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

const fn opponent(seat: u8) -> u8 {
    1 - seat
}

fn hand_has(state: &CoupState, seat: u8, card: Character) -> bool {
    state.hands[seat as usize].contains(&card)
}

/// The claimant proved they hold `card`: return it to the deck, shuffle, and
/// draw a replacement, so the card stays hidden.
fn redraw(state: &mut CoupState, seat: u8, card: Character) {
    let hand = &mut state.hands[seat as usize];
    if let Some(pos) = hand.iter().position(|&c| c == card) {
        hand.remove(pos);
        state.deck.push(card);
        state.rng.shuffle(&mut state.deck);
        if let Some(drawn) = state.deck.pop() {
            state.hands[seat as usize].push(drawn);
        }
    }
}

/// Applies a resolved action's effect, then either ends the turn or opens the
/// `Lose` decision point the effect requires.
fn resolve_action(state: &mut CoupState, action: Action, actor: u8) {
    let target = opponent(actor);
    match action {
        Action::ForeignAid => {
            state.coins[actor as usize] += 2;
            state.end_turn();
        }
        Action::Tax => {
            state.coins[actor as usize] += 3;
            state.end_turn();
        }
        Action::Steal => {
            let amount = state.coins[target as usize].min(2);
            state.coins[target as usize] -= amount;
            state.coins[actor as usize] += amount;
            state.end_turn();
        }
        Action::Assassinate => {
            state.phase = Phase::Lose {
                who: target,
                resume: Resume::EndTurn,
            };
        }
        _ => state.end_turn(),
    }
}

fn apply_choose(state: &mut CoupState, action: Action) {
    let actor = state.current;
    match action {
        Action::Income => {
            state.coins[actor as usize] += 1;
            state.end_turn();
        }
        Action::Coup => {
            state.coins[actor as usize] -= 7;
            state.phase = Phase::Lose {
                who: opponent(actor),
                resume: Resume::EndTurn,
            };
        }
        Action::ForeignAid => {
            state.phase = Phase::Respond {
                pending: Pending {
                    action,
                    actor,
                    claim: None,
                    block_options: vec![Character::Duke],
                },
            };
        }
        Action::Tax => {
            state.phase = Phase::Respond {
                pending: Pending {
                    action,
                    actor,
                    claim: Some(Character::Duke),
                    block_options: Vec::new(),
                },
            };
        }
        Action::Assassinate => {
            state.coins[actor as usize] -= 3;
            state.phase = Phase::Respond {
                pending: Pending {
                    action,
                    actor,
                    claim: Some(Character::Assassin),
                    block_options: vec![Character::Contessa],
                },
            };
        }
        Action::Steal => {
            state.phase = Phase::Respond {
                pending: Pending {
                    action,
                    actor,
                    claim: Some(Character::Captain),
                    block_options: vec![Character::Captain, Character::Ambassador],
                },
            };
        }
        _ => {}
    }
}

fn apply_respond(state: &mut CoupState, pending: Pending, action: Action) {
    let responder = opponent(pending.actor);
    match action {
        Action::Pass => resolve_action(state, pending.action, pending.actor),
        Action::Challenge => {
            let claim = pending
                .claim
                .expect("Challenge is only legal against a claim");
            if hand_has(state, pending.actor, claim) {
                // Claim was true: the challenger loses an influence, the actor
                // redraws the proven card, then the action resolves.
                redraw(state, pending.actor, claim);
                state.phase = Phase::Lose {
                    who: responder,
                    resume: Resume::ApplyThenEnd {
                        action: pending.action,
                        actor: pending.actor,
                    },
                };
            } else {
                // Caught bluffing: the actor loses an influence, action fizzles.
                state.phase = Phase::Lose {
                    who: pending.actor,
                    resume: Resume::EndTurn,
                };
            }
        }
        Action::Block(block_as) => {
            state.phase = Phase::RespondToBlock {
                pending,
                blocker: responder,
                block_as,
            };
        }
        _ => {}
    }
}

fn apply_respond_block(
    state: &mut CoupState,
    pending: &Pending,
    blocker: u8,
    block_as: Character,
    action: Action,
) {
    match action {
        Action::Pass => state.end_turn(), // block stands; action fizzles
        Action::Challenge => {
            if hand_has(state, blocker, block_as) {
                // Block was true: the actor loses an influence, blocker redraws,
                // action stays blocked.
                redraw(state, blocker, block_as);
                state.phase = Phase::Lose {
                    who: pending.actor,
                    resume: Resume::EndTurn,
                };
            } else {
                // Block was a bluff: the blocker loses an influence, then the
                // original action resolves.
                state.phase = Phase::Lose {
                    who: blocker,
                    resume: Resume::ApplyThenEnd {
                        action: pending.action,
                        actor: pending.actor,
                    },
                };
            }
        }
        _ => {}
    }
}

fn apply_lose(state: &mut CoupState, who: u8, resume: Resume, action: Action) {
    let Action::Lose(index) = action else {
        return;
    };
    let seat = who as usize;
    if index >= state.hands[seat].len() {
        return;
    }
    let card = state.hands[seat].remove(index);
    state.lost[seat].push(card);

    if state.hands[seat].is_empty() {
        // One seat eliminated ends 2-player Coup.
        state.phase = Phase::GameOver;
        return;
    }
    match resume {
        Resume::EndTurn => state.end_turn(),
        Resume::ApplyThenEnd { action, actor } => resolve_action(state, action, actor),
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
        let seat = match &state.phase {
            Phase::ChooseAction => state.current,
            Phase::Respond { pending } => opponent(pending.actor),
            Phase::RespondToBlock { pending, .. } => pending.actor,
            Phase::Lose { who, .. } => *who,
            Phase::GameOver => return ActivePlayers::none(),
        };
        ActivePlayers::one(PlayerId::new(u32::from(seat)))
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        let seat = player.index();
        match &state.phase {
            Phase::ChooseAction if seat == u32::from(state.current) => {
                let coins = state.coins[seat as usize];
                if coins >= 10 {
                    return vec![Action::Coup];
                }
                let mut actions = vec![
                    Action::Income,
                    Action::ForeignAid,
                    Action::Tax,
                    Action::Steal,
                ];
                if coins >= 3 {
                    actions.push(Action::Assassinate);
                }
                if coins >= 7 {
                    actions.push(Action::Coup);
                }
                actions
            }
            Phase::Respond { pending } if seat == u32::from(opponent(pending.actor)) => {
                let mut actions = vec![Action::Pass];
                if pending.claim.is_some() {
                    actions.push(Action::Challenge);
                }
                for &character in &pending.block_options {
                    actions.push(Action::Block(character));
                }
                actions
            }
            Phase::RespondToBlock { pending, .. } if seat == u32::from(pending.actor) => {
                vec![Action::Pass, Action::Challenge]
            }
            Phase::Lose { who, .. } if seat == u32::from(*who) => (0..state.hands[*who as usize]
                .len())
                .map(Action::Lose)
                .collect(),
            _ => Vec::new(),
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        match state.phase.clone() {
            Phase::ChooseAction => apply_choose(state, action),
            Phase::Respond { pending } => apply_respond(state, pending, action),
            Phase::RespondToBlock {
                pending,
                blocker,
                block_as,
            } => {
                apply_respond_block(state, &pending, blocker, block_as, action);
            }
            Phase::Lose { who, resume } => apply_lose(state, who, resume, action),
            Phase::GameOver => {}
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.is_over()
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let seat = player.index() as usize;
        let other = 1 - seat;
        match (state.hands[seat].is_empty(), state.hands[other].is_empty()) {
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
#[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
mod tests {
    use super::{Action, CHARACTERS, Character, Coup, CoupState, Phase};
    use turnbase::{Game, PlayerId, Prng};

    use Action::{Block, Challenge, Lose, Pass};
    use Character::{Ambassador, Assassin, Captain, Contessa, Duke};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Builds a state with chosen hands and coins; the deck is the remaining
    /// cards. P0 to act.
    fn rigged(hands: [Vec<Character>; 2], coins: [u8; 2]) -> CoupState {
        let mut deck = Vec::new();
        for character in CHARACTERS {
            for _ in 0..3 {
                deck.push(character);
            }
        }
        for hand in &hands {
            for &card in hand {
                if let Some(pos) = deck.iter().position(|&c| c == card) {
                    deck.remove(pos);
                }
            }
        }
        CoupState {
            coins,
            hands,
            lost: [Vec::new(), Vec::new()],
            deck,
            current: 0,
            phase: Phase::ChooseAction,
            rng: Prng::new(0),
        }
    }

    #[test]
    fn tax_unchallenged_gains_three() {
        let game = Coup;
        let mut state = rigged([vec![Duke, Captain], vec![Contessa, Assassin]], [2, 2]);
        game.apply(&mut state, P0, Action::Tax);
        game.apply(&mut state, P1, Pass);
        assert_eq!(state.coins(0), 5);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn tax_bluff_caught_costs_the_bluffer_an_influence() {
        let game = Coup;
        // P0 claims Tax without a Duke.
        let mut state = rigged([vec![Captain, Contessa], vec![Duke, Assassin]], [2, 2]);
        game.apply(&mut state, P0, Action::Tax);
        game.apply(&mut state, P1, Challenge);
        game.apply(&mut state, P0, Lose(0)); // bluffer loses
        assert_eq!(state.coins(0), 2, "no coins gained on a caught bluff");
        assert_eq!(state.influence(0), 1);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn failed_challenge_costs_the_challenger_and_the_action_resolves() {
        let game = Coup;
        // P0 really has a Duke.
        let mut state = rigged([vec![Duke, Captain], vec![Contessa, Assassin]], [2, 2]);
        game.apply(&mut state, P0, Action::Tax);
        game.apply(&mut state, P1, Challenge);
        game.apply(&mut state, P1, Lose(0)); // challenger loses
        assert_eq!(state.influence(1), 1);
        assert_eq!(state.influence(0), 2, "actor kept two influence via redraw");
        assert_eq!(state.coins(0), 5, "the tax still resolved");
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn foreign_aid_unblocked_gains_two() {
        let game = Coup;
        let mut state = rigged([vec![Captain, Contessa], vec![Assassin, Assassin]], [2, 2]);
        game.apply(&mut state, P0, Action::ForeignAid);
        game.apply(&mut state, P1, Pass);
        assert_eq!(state.coins(0), 4);
    }

    #[test]
    fn foreign_aid_blocked_by_duke_gains_nothing() {
        let game = Coup;
        let mut state = rigged([vec![Captain, Contessa], vec![Duke, Assassin]], [2, 2]);
        game.apply(&mut state, P0, Action::ForeignAid);
        game.apply(&mut state, P1, Block(Duke));
        game.apply(&mut state, P0, Pass); // accept the block
        assert_eq!(state.coins(0), 2);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn assassinate_makes_the_target_lose_an_influence() {
        let game = Coup;
        let mut state = rigged([vec![Assassin, Captain], vec![Contessa, Duke]], [3, 2]);
        game.apply(&mut state, P0, Action::Assassinate);
        game.apply(&mut state, P1, Pass);
        game.apply(&mut state, P1, Lose(0));
        assert_eq!(state.coins(0), 0, "assassination costs three");
        assert_eq!(state.influence(1), 1);
    }

    #[test]
    fn assassinate_blocked_by_contessa_spares_the_target() {
        let game = Coup;
        let mut state = rigged([vec![Assassin, Captain], vec![Contessa, Duke]], [3, 2]);
        game.apply(&mut state, P0, Action::Assassinate);
        game.apply(&mut state, P1, Block(Contessa));
        game.apply(&mut state, P0, Pass);
        assert_eq!(state.influence(1), 2, "the target was spared");
        assert_eq!(state.coins(0), 0, "but the coins were still spent");
    }

    #[test]
    fn challenging_a_real_assassin_can_cost_the_game() {
        let game = Coup;
        let mut state = rigged([vec![Assassin, Captain], vec![Contessa, Duke]], [3, 2]);
        game.apply(&mut state, P0, Action::Assassinate);
        game.apply(&mut state, P1, Challenge); // P0 has the Assassin
        game.apply(&mut state, P1, Lose(0)); // challenge penalty
        game.apply(&mut state, P1, Lose(0)); // then the assassination
        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, P0), 1.0);
        assert_eq!(game.reward(&state, P1), -1.0);
    }

    #[test]
    fn steal_takes_two_coins() {
        let game = Coup;
        let mut state = rigged([vec![Captain, Duke], vec![Contessa, Assassin]], [2, 5]);
        game.apply(&mut state, P0, Action::Steal);
        game.apply(&mut state, P1, Pass);
        assert_eq!(state.coins(0), 4);
        assert_eq!(state.coins(1), 3);
    }

    #[test]
    fn steal_blocked_by_ambassador_takes_nothing() {
        let game = Coup;
        let mut state = rigged([vec![Captain, Duke], vec![Ambassador, Assassin]], [2, 5]);
        game.apply(&mut state, P0, Action::Steal);
        game.apply(&mut state, P1, Block(Ambassador));
        game.apply(&mut state, P0, Pass);
        assert_eq!(state.coins(0), 2);
        assert_eq!(state.coins(1), 5);
    }

    #[test]
    fn random_self_play_always_terminates() {
        let game = Coup;
        for seed in 0..50 {
            let mut state = game.new_initial_state(seed);
            let mut rng = Prng::new(seed ^ 0xABCD);
            let mut steps = 0;
            while !game.is_terminal(&state) {
                let player = game.active_players(&state).iter().next().unwrap();
                let actions = game.legal_actions(&state, player);
                let index = usize::try_from(rng.below(actions.len() as u64)).unwrap();
                game.apply(&mut state, player, actions[index]);
                steps += 1;
                assert!(steps < 10_000, "seed {seed} did not terminate");
            }
            // Exactly one seat should be standing.
            assert_ne!(state.influence(0) == 0, state.influence(1) == 0);
        }
    }
}
