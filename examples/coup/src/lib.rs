//! Coup (2-4 players): a bluffing game of hidden influence.
//!
//! The response-window validation from `ARCHITECTURE.md`, at its full scale. A
//! turn is a small state machine of decision points: a declared character
//! action opens a `Respond` window that every other living seat passes through
//! in turn order (each may pass, challenge, or block), and a block opens a
//! `RespondToBlock` window that every seat except the blocker passes through.
//! Every window is a `Phase` whose `active_players`/`legal_actions` are computed
//! from a queue of remaining responders on the game's own state. Generalizing
//! from two players to four is just letting that queue hold more than one seat;
//! no engine machinery changes.
//!
//! Three state zones: public fields, each seat's face-down `hands` (private),
//! and the `deck` (hidden from everyone). `view` returns public + own hand.
//! Multi-player rules modeled: any seat may challenge a claim; only the target
//! may block Assassinate/Steal; any seat may block Foreign Aid; a block may be
//! challenged by anyone but the blocker. Challenge priority is sequentialized
//! into turn order (see `.matan/coup-plan.md`).

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Game, Pile, PlayerId, Prng};

#[cfg(feature = "printable")]
mod ui;

/// A character card. The deck holds three of each.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
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

/// A move. The legal set is phase-dependent. Targeted actions name their target
/// seat.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// +1 coin. Uncontested.
    Income,
    /// +2 coins. Blockable by Duke (any seat).
    ForeignAid,
    /// Pay 7; the target loses an influence. Forced at 10+. Uncontested.
    Coup(u8),
    /// Claim Duke for +3 coins. Challengeable.
    Tax,
    /// Claim Assassin, pay 3; the target loses an influence. Challengeable,
    /// blockable by the target with Contessa.
    Assassinate(u8),
    /// Claim Captain, take 2 coins from the target. Challengeable, blockable by
    /// the target with Captain or Ambassador.
    Steal(u8),
    /// Claim Ambassador, draw two and choose which to keep. Challengeable.
    Exchange,
    /// Return the card at this pool index to the deck (during an exchange).
    Return(usize),
    /// Allow the pending action or block to stand.
    Pass,
    /// Challenge the pending claim.
    Challenge,
    /// Block the pending action by claiming this character.
    Block(Character),
    /// Reveal and discard the influence at this hand index.
    Lose(usize),
}

/// The action being resolved and the seats that still owe a response.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct Pending {
    action: Action,
    actor: u8,
    claim: Option<Character>,
    to_respond: Vec<u8>,
}

/// What to run once a `Lose` is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Resume {
    EndTurn,
    ApplyThenEnd { action: Action, actor: u8 },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
enum Phase {
    ChooseAction,
    Respond {
        pending: Pending,
    },
    RespondToBlock {
        action: Action,
        actor: u8,
        blocker: u8,
        block_as: Character,
        to_respond: Vec<u8>,
    },
    Lose {
        who: u8,
        resume: Resume,
    },
    ExchangeReturn {
        player: u8,
        pool: Vec<Character>,
        returns_left: u8,
    },
    GameOver,
}

/// A Coup position for 2-4 seats.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CoupState {
    coins: Vec<u8>,
    hands: Vec<Vec<Character>>,
    lost: Vec<Vec<Character>>,
    deck: Pile<Character>,
    current: u8,
    seats: u8,
    phase: Phase,
    rng: Prng,
}

impl CoupState {
    /// Coins held by seat `player`.
    #[must_use]
    pub fn coins(&self, player: usize) -> u8 {
        self.coins[player]
    }

    /// Number of face-down influence cards seat `player` still holds.
    #[must_use]
    pub fn influence(&self, player: usize) -> usize {
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

    /// The number of seats.
    #[must_use]
    pub const fn seats(&self) -> u8 {
        self.seats
    }

    /// Whether the match has ended.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        matches!(self.phase, Phase::GameOver)
    }

    fn end_turn(&mut self) {
        let mut seat = (self.current + 1) % self.seats;
        while self.hands[seat as usize].is_empty() {
            seat = (seat + 1) % self.seats;
        }
        self.current = seat;
        self.phase = Phase::ChooseAction;
    }
}

/// What `viewer` observes: the public fields plus their own hand.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CoupView {
    /// Coins held by each seat.
    pub coins: Vec<u8>,
    /// Revealed cards of each seat.
    pub lost: Vec<Vec<Character>>,
    /// Face-down influence count of each seat.
    pub influence: Vec<usize>,
    /// Cards remaining in the hidden deck (count only).
    pub deck_size: usize,
    /// The seat to move.
    pub current: u8,
    /// The viewer's own hand, or empty for a seatless spectator.
    pub own_hand: Vec<Character>,
    /// Whether the match is over.
    pub over: bool,
    /// What is currently being decided, and by whom.
    pub pending: PendingView,
}

/// What is currently being decided, and by whom.
///
/// A UI-facing summary of the internal turn state machine (declared action,
/// claim, who must respond) that `Phase` (private) tracks but does not
/// itself expose.
///
/// Every variant names the seat currently owed a decision, so a consumer
/// does not need to separately call [`Game::active_players`] to know who
/// that is and cross-reference it against the pending context.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PendingView {
    /// `actor` is choosing their turn's action.
    ChooseAction {
        /// The seat about to act.
        actor: u8,
    },
    /// `actor` has declared `action` (claiming `claim`, if any); `responder`
    /// may pass, challenge, or (for a blockable action) block.
    Respond {
        /// The seat whose action is pending.
        actor: u8,
        /// The declared action.
        action: Action,
        /// The character claimed to justify it, or `None` for an
        /// unclaimed action like Foreign Aid.
        claim: Option<Character>,
        /// The seat currently asked to respond.
        responder: u8,
    },
    /// `blocker` has claimed `block_as` to block `actor`'s `action`;
    /// `responder` may pass or challenge the block.
    RespondToBlock {
        /// The seat whose action was blocked.
        actor: u8,
        /// The seat claiming the block.
        blocker: u8,
        /// The action being blocked.
        action: Action,
        /// The character claimed to justify the block.
        block_as: Character,
        /// The seat currently asked to respond to the block.
        responder: u8,
    },
    /// `who` must reveal and discard one influence card.
    Lose {
        /// The seat losing influence.
        who: u8,
    },
    /// `player` is exchanging. `pool` holds the drawn-plus-kept cards under
    /// consideration for [`Action::Return`], populated only in `player`'s
    /// own view (empty for every other viewer, including spectators) —
    /// `player`'s hand is briefly empty for the duration of the exchange, so
    /// this is the only place those cards are visible at all.
    ExchangeReturn {
        /// The seat exchanging.
        player: u8,
        /// The exchange pool, visible only to `player`'s own view.
        pool: Vec<Character>,
        /// How many cards must still be returned.
        returns_left: u8,
    },
    /// The match has ended.
    GameOver,
}

/// The rules of Coup for a chosen number of seats.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Coup {
    seats: u8,
}

impl Coup {
    /// Creates a game for `seats` players (2-4).
    #[must_use]
    pub const fn new(seats: u8) -> Self {
        Self { seats }
    }
}

impl Default for Coup {
    fn default() -> Self {
        Self::new(2)
    }
}

#[allow(clippy::cast_possible_truncation)] // seat indices are 0..=3
const fn seat_of(player: PlayerId) -> u8 {
    player.index() as u8
}

fn alive(state: &CoupState, seat: u8) -> bool {
    !state.hands[seat as usize].is_empty()
}

fn count_alive(state: &CoupState) -> usize {
    state.hands.iter().filter(|hand| !hand.is_empty()).count()
}

/// Living seats other than `actor`, in turn order starting after `actor`.
fn responders(state: &CoupState, actor: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut seat = (actor + 1) % state.seats;
    while seat != actor {
        if alive(state, seat) {
            out.push(seat);
        }
        seat = (seat + 1) % state.seats;
    }
    out
}

/// Living seats able to challenge a block: everyone but the blocker, in turn
/// order starting from the actor.
fn block_challengers(state: &CoupState, actor: u8, blocker: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut seat = actor;
    loop {
        if seat != blocker && alive(state, seat) {
            out.push(seat);
        }
        seat = (seat + 1) % state.seats;
        if seat == actor {
            break;
        }
    }
    out
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
        state.deck.put(card);
        state.deck.shuffle(&mut state.rng);
        if let Some(drawn) = state.deck.draw() {
            state.hands[seat as usize].push(drawn);
        }
    }
}

/// Applies a resolved action's effect, then either ends the turn or opens the
/// decision point the effect requires.
fn resolve_action(state: &mut CoupState, action: Action, actor: u8) {
    match action {
        Action::ForeignAid => {
            state.coins[actor as usize] += 2;
            state.end_turn();
        }
        Action::Tax => {
            state.coins[actor as usize] += 3;
            state.end_turn();
        }
        Action::Steal(target) => {
            let amount = state.coins[target as usize].min(2);
            state.coins[target as usize] -= amount;
            state.coins[actor as usize] += amount;
            state.end_turn();
        }
        Action::Assassinate(target) => {
            if alive(state, target) {
                state.phase = Phase::Lose {
                    who: target,
                    resume: Resume::EndTurn,
                };
            } else {
                state.end_turn();
            }
        }
        Action::Exchange => start_exchange(state, actor),
        _ => state.end_turn(),
    }
}

fn start_exchange(state: &mut CoupState, actor: u8) {
    // Draw up to two, pool them with the current hand, then return as many as
    // were drawn. The hand is emptied into the pool for the decision.
    let mut pool = std::mem::take(&mut state.hands[actor as usize]);
    let mut drawn = 0u8;
    for _ in 0..2 {
        if let Some(card) = state.deck.draw() {
            pool.push(card);
            drawn += 1;
        }
    }
    if drawn == 0 {
        state.hands[actor as usize] = pool;
        state.end_turn();
    } else {
        state.phase = Phase::ExchangeReturn {
            player: actor,
            pool,
            returns_left: drawn,
        };
    }
}

fn apply_exchange_return(
    state: &mut CoupState,
    player: u8,
    mut pool: Vec<Character>,
    returns_left: u8,
    action: Action,
) {
    let Action::Return(index) = action else {
        return;
    };
    if index >= pool.len() {
        return;
    }
    state.deck.put(pool.remove(index));
    let remaining = returns_left - 1;
    if remaining == 0 {
        state.deck.shuffle(&mut state.rng);
        state.hands[player as usize] = pool;
        state.end_turn();
    } else {
        state.phase = Phase::ExchangeReturn {
            player,
            pool,
            returns_left: remaining,
        };
    }
}

fn open_response(state: &mut CoupState, action: Action, actor: u8, claim: Option<Character>) {
    let to_respond = responders(state, actor);
    if to_respond.is_empty() {
        resolve_action(state, action, actor);
    } else {
        state.phase = Phase::Respond {
            pending: Pending {
                action,
                actor,
                claim,
                to_respond,
            },
        };
    }
}

fn apply_choose(state: &mut CoupState, action: Action) {
    let actor = state.current;
    match action {
        Action::Income => {
            state.coins[actor as usize] += 1;
            state.end_turn();
        }
        Action::Coup(target) => {
            state.coins[actor as usize] -= 7;
            state.phase = Phase::Lose {
                who: target,
                resume: Resume::EndTurn,
            };
        }
        Action::ForeignAid => open_response(state, action, actor, None),
        Action::Tax => open_response(state, action, actor, Some(Character::Duke)),
        Action::Assassinate(_) => {
            state.coins[actor as usize] -= 3;
            open_response(state, action, actor, Some(Character::Assassin));
        }
        Action::Steal(_) => open_response(state, action, actor, Some(Character::Captain)),
        Action::Exchange => open_response(state, action, actor, Some(Character::Ambassador)),
        _ => {}
    }
}

fn apply_respond(state: &mut CoupState, mut pending: Pending, action: Action) {
    match action {
        Action::Pass => {
            pending.to_respond.remove(0);
            if pending.to_respond.is_empty() {
                resolve_action(state, pending.action, pending.actor);
            } else {
                state.phase = Phase::Respond { pending };
            }
        }
        Action::Challenge => {
            let claim = pending
                .claim
                .expect("Challenge is only legal against a claim");
            let challenger = pending.to_respond[0];
            if hand_has(state, pending.actor, claim) {
                redraw(state, pending.actor, claim);
                state.phase = Phase::Lose {
                    who: challenger,
                    resume: Resume::ApplyThenEnd {
                        action: pending.action,
                        actor: pending.actor,
                    },
                };
            } else {
                state.phase = Phase::Lose {
                    who: pending.actor,
                    resume: Resume::EndTurn,
                };
            }
        }
        Action::Block(block_as) => {
            let blocker = pending.to_respond[0];
            let to_respond = block_challengers(state, pending.actor, blocker);
            if to_respond.is_empty() {
                state.end_turn();
            } else {
                state.phase = Phase::RespondToBlock {
                    action: pending.action,
                    actor: pending.actor,
                    blocker,
                    block_as,
                    to_respond,
                };
            }
        }
        _ => {}
    }
}

fn apply_respond_block(
    state: &mut CoupState,
    action: Action,
    actor: u8,
    blocker: u8,
    block_as: Character,
    mut to_respond: Vec<u8>,
    response: Action,
) {
    match response {
        Action::Pass => {
            to_respond.remove(0);
            if to_respond.is_empty() {
                state.end_turn(); // block stands; action fizzles
            } else {
                state.phase = Phase::RespondToBlock {
                    action,
                    actor,
                    blocker,
                    block_as,
                    to_respond,
                };
            }
        }
        Action::Challenge => {
            let challenger = to_respond[0];
            if hand_has(state, blocker, block_as) {
                redraw(state, blocker, block_as);
                state.phase = Phase::Lose {
                    who: challenger,
                    resume: Resume::EndTurn,
                };
            } else {
                state.phase = Phase::Lose {
                    who: blocker,
                    resume: Resume::ApplyThenEnd { action, actor },
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

    if count_alive(state) == 1 {
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
        let seats = self.seats as usize;
        let mut rng = Prng::new(seed);
        let mut deck = Pile::new();
        for character in CHARACTERS {
            for _ in 0..3 {
                deck.put(character);
            }
        }
        deck.shuffle(&mut rng);

        let mut hands = vec![Vec::new(); seats];
        for hand in &mut hands {
            hand.push(deck.draw().unwrap());
            hand.push(deck.draw().unwrap());
        }

        CoupState {
            coins: vec![2; seats],
            hands,
            lost: vec![Vec::new(); seats],
            deck,
            current: 0,
            seats: self.seats,
            phase: Phase::ChooseAction,
            rng,
        }
    }

    fn num_players(&self) -> usize {
        self.seats as usize
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        let seat = match &state.phase {
            Phase::ChooseAction => state.current,
            Phase::Respond { pending } => pending.to_respond[0],
            Phase::RespondToBlock { to_respond, .. } => to_respond[0],
            Phase::Lose { who, .. } => *who,
            Phase::ExchangeReturn { player, .. } => *player,
            Phase::GameOver => return ActivePlayers::none(),
        };
        ActivePlayers::one(PlayerId::new(u32::from(seat)))
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        let seat = seat_of(player);
        match &state.phase {
            Phase::ChooseAction if seat == state.current => choose_actions(state, seat),
            Phase::Respond { pending } if seat == pending.to_respond[0] => {
                respond_actions(pending, seat)
            }
            Phase::RespondToBlock { to_respond, .. } if seat == to_respond[0] => {
                vec![Action::Pass, Action::Challenge]
            }
            Phase::Lose { who, .. } if seat == *who => (0..state.hands[*who as usize].len())
                .map(Action::Lose)
                .collect(),
            Phase::ExchangeReturn { player, pool, .. } if seat == *player => {
                (0..pool.len()).map(Action::Return).collect()
            }
            _ => Vec::new(),
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        match state.phase.clone() {
            Phase::ChooseAction => apply_choose(state, action),
            Phase::Respond { pending } => apply_respond(state, pending, action),
            Phase::RespondToBlock {
                action: pending_action,
                actor,
                blocker,
                block_as,
                to_respond,
            } => apply_respond_block(
                state,
                pending_action,
                actor,
                blocker,
                block_as,
                to_respond,
                action,
            ),
            Phase::Lose { who, resume } => apply_lose(state, who, resume, action),
            Phase::ExchangeReturn {
                player,
                pool,
                returns_left,
            } => apply_exchange_return(state, player, pool, returns_left, action),
            Phase::GameOver => {}
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.is_over()
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let seat = seat_of(player);
        if !alive(state, seat) {
            -1.0
        } else if count_alive(state) == 1 {
            1.0
        } else {
            0.0
        }
    }

    fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
        let own_hand = viewer
            .map(|p| state.hands[p.index() as usize].clone())
            .unwrap_or_default();
        CoupView {
            coins: state.coins.clone(),
            lost: state.lost.clone(),
            influence: state.hands.iter().map(Vec::len).collect(),
            deck_size: state.deck.len(),
            current: state.current,
            own_hand,
            over: state.is_over(),
            pending: pending_view(state, viewer),
        }
    }
}

/// Builds the [`PendingView`] `viewer` sees for `state`'s current phase.
fn pending_view(state: &CoupState, viewer: Option<PlayerId>) -> PendingView {
    match &state.phase {
        Phase::ChooseAction => PendingView::ChooseAction {
            actor: state.current,
        },
        Phase::Respond { pending } => PendingView::Respond {
            actor: pending.actor,
            action: pending.action,
            claim: pending.claim,
            responder: pending.to_respond[0],
        },
        Phase::RespondToBlock {
            action,
            actor,
            blocker,
            block_as,
            to_respond,
        } => PendingView::RespondToBlock {
            actor: *actor,
            blocker: *blocker,
            action: *action,
            block_as: *block_as,
            responder: to_respond[0],
        },
        Phase::Lose { who, .. } => PendingView::Lose { who: *who },
        Phase::ExchangeReturn {
            player,
            pool,
            returns_left,
        } => PendingView::ExchangeReturn {
            player: *player,
            pool: if viewer == Some(PlayerId::new(u32::from(*player))) {
                pool.clone()
            } else {
                Vec::new()
            },
            returns_left: *returns_left,
        },
        Phase::GameOver => PendingView::GameOver,
    }
}

impl turnbase::Determinize for Coup {
    /// Resamples the cards the observer cannot see. Their own hand, every
    /// revealed (lost) card, and the exchange pool when they own it are known
    /// and left untouched; the other seats' hidden hands, a foreign exchange
    /// pool, and the deck are refilled from the unseen cards. Card counts (and
    /// so the observer's whole `view`) are preserved exactly.
    fn determinize(&self, state: &CoupState, observer: PlayerId, rng: &mut Prng) -> CoupState {
        let obs = observer.index() as usize;
        let mut next = state.clone();

        // Start from the full deck (three of each) and remove what's known.
        let mut bag: Vec<Character> = CHARACTERS.iter().flat_map(|&c| [c, c, c]).collect();
        for pile in &next.lost {
            remove_each(&mut bag, pile);
        }
        remove_each(&mut bag, &next.hands[obs]);
        let owns_pool = matches!(
            &next.phase,
            Phase::ExchangeReturn { player, .. } if *player as usize == obs
        );
        if owns_pool && let Phase::ExchangeReturn { pool, .. } = &next.phase {
            remove_each(&mut bag, pool);
        }

        // Deal the unseen cards back into the hidden slots.
        rng.shuffle(&mut bag);
        for seat in 0..next.hands.len() {
            if seat != obs {
                let count = next.hands[seat].len();
                next.hands[seat] = deal(&mut bag, count);
            }
        }
        if !owns_pool && let Phase::ExchangeReturn { pool, .. } = &mut next.phase {
            let count = pool.len();
            *pool = deal(&mut bag, count);
        }
        let deck_size = next.deck.len();
        next.deck = Pile::from_items(deal(&mut bag, deck_size));
        next
    }
}

/// Removes one copy of each card in `cards` from `bag`.
fn remove_each(bag: &mut Vec<Character>, cards: &[Character]) {
    for card in cards {
        if let Some(index) = bag.iter().position(|c| c == card) {
            bag.swap_remove(index);
        }
    }
}

/// Removes and returns `count` cards from the top of `bag`.
fn deal(bag: &mut Vec<Character>, count: usize) -> Vec<Character> {
    let mut dealt = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(card) = bag.pop() {
            dealt.push(card);
        }
    }
    dealt
}

fn choose_actions(state: &CoupState, seat: u8) -> Vec<Action> {
    let coins = state.coins[seat as usize];
    let targets = responders(state, seat);
    if coins >= 10 {
        return targets.into_iter().map(Action::Coup).collect();
    }
    let mut actions = vec![
        Action::Income,
        Action::ForeignAid,
        Action::Tax,
        Action::Exchange,
    ];
    for &target in &targets {
        actions.push(Action::Steal(target));
    }
    if coins >= 3 {
        for &target in &targets {
            actions.push(Action::Assassinate(target));
        }
    }
    if coins >= 7 {
        for &target in &targets {
            actions.push(Action::Coup(target));
        }
    }
    actions
}

fn respond_actions(pending: &Pending, responder: u8) -> Vec<Action> {
    let mut actions = vec![Action::Pass];
    if pending.claim.is_some() {
        actions.push(Action::Challenge);
    }
    match pending.action {
        Action::ForeignAid => actions.push(Action::Block(Character::Duke)),
        Action::Assassinate(target) if target == responder => {
            actions.push(Action::Block(Character::Contessa));
        }
        Action::Steal(target) if target == responder => {
            actions.push(Action::Block(Character::Captain));
            actions.push(Action::Block(Character::Ambassador));
        }
        _ => {}
    }
    actions
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
mod tests {
    use super::Action::{self, Block, Challenge, Exchange, Lose, Pass, Steal, Tax};
    use super::{CHARACTERS, Character, Coup, CoupState, Phase};
    use Character::{Ambassador, Assassin, Captain, Contessa, Duke};
    use turnbase::{Determinize, Game, Pile, PlayerId, Prng};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);
    const P2: PlayerId = PlayerId::new(2);

    /// Builds a state with the given hands and coins; the deck is the leftover
    /// cards. Seat 0 to act.
    fn rigged(hands: Vec<Vec<Character>>, coins: Vec<u8>) -> CoupState {
        let seats = u8::try_from(hands.len()).unwrap();
        let mut deck = Pile::new();
        for character in CHARACTERS {
            for _ in 0..3 {
                deck.put(character);
            }
        }
        for hand in &hands {
            for &card in hand {
                deck.remove_item(&card);
            }
        }
        let lost = vec![Vec::new(); hands.len()];
        CoupState {
            coins,
            hands,
            lost,
            deck,
            current: 0,
            seats,
            phase: Phase::ChooseAction,
            rng: Prng::new(0),
        }
    }

    /// Total cards in play (hands + revealed + deck), grouped by character.
    fn card_counts(state: &CoupState) -> [u8; 5] {
        let mut counts = [0u8; 5];
        let index = |c: Character| CHARACTERS.iter().position(|&x| x == c).unwrap();
        for hand in (0..state.seats() as usize).map(|s| state.hand(s)) {
            for &c in hand {
                counts[index(c)] += 1;
            }
        }
        for lost in (0..state.seats() as usize).map(|s| state.lost(s)) {
            for &c in lost {
                counts[index(c)] += 1;
            }
        }
        for &c in &state.deck {
            counts[index(c)] += 1;
        }
        // Mid-exchange, drawn cards live transiently in the phase pool.
        if let Phase::ExchangeReturn { pool, .. } = &state.phase {
            for &c in pool {
                counts[index(c)] += 1;
            }
        }
        counts
    }

    fn play_random(game: Coup, seed: u64) -> CoupState {
        let mut state = game.new_initial_state(seed);
        let mut rng = Prng::new(seed ^ 0xABCD);
        let mut steps = 0;
        while !game.is_terminal(&state) {
            // Card conservation holds at every step.
            assert_eq!(
                card_counts(&state),
                [3; 5],
                "cards not conserved (seed {seed})"
            );
            let player = game.active_players(&state).iter().next().unwrap();
            let actions = game.legal_actions(&state, player);
            let index = usize::try_from(rng.below(actions.len() as u64)).unwrap();
            game.apply(&mut state, player, actions[index]);
            steps += 1;
            assert!(steps < 20_000, "seed {seed} did not terminate");
        }
        state
    }

    #[test]
    fn determinize_stays_in_the_information_set() {
        // Every determinization must preserve exactly what the observer sees
        // (so their `view` is unchanged) and conserve all fifteen cards. Walk
        // random games and check both for every observer at every decision.
        let game = Coup::new(3);
        for seed in 0..20 {
            let mut state = game.new_initial_state(seed);
            let mut walk = Prng::new(seed ^ 0x1234);
            let mut resample = Prng::new(seed ^ 0x9999);
            let mut steps = 0;
            while !game.is_terminal(&state) {
                for obs in 0..game.num_players() {
                    let observer = PlayerId::new(u32::try_from(obs).unwrap());
                    let world = game.determinize(&state, observer, &mut resample);
                    assert_eq!(
                        game.view(&world, Some(observer)),
                        game.view(&state, Some(observer)),
                        "determinization changed observer {obs}'s view (seed {seed})"
                    );
                    assert_eq!(
                        card_counts(&world),
                        [3; 5],
                        "determinization did not conserve cards (seed {seed})"
                    );
                }
                let player = game.active_players(&state).iter().next().unwrap();
                let actions = game.legal_actions(&state, player);
                let index = usize::try_from(walk.below(actions.len() as u64)).unwrap();
                game.apply(&mut state, player, actions[index]);
                steps += 1;
                assert!(steps < 20_000, "seed {seed} did not terminate");
            }
        }
    }

    #[test]
    fn tax_unchallenged_gains_three() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Duke, Captain], vec![Contessa, Assassin]],
            vec![2, 2],
        );
        game.apply(&mut state, P0, Tax);
        game.apply(&mut state, P1, Pass);
        assert_eq!(state.coins(0), 5);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn tax_bluff_caught_costs_the_bluffer() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Captain, Contessa], vec![Duke, Assassin]],
            vec![2, 2],
        );
        game.apply(&mut state, P0, Tax);
        game.apply(&mut state, P1, Challenge);
        game.apply(&mut state, P0, Lose(0));
        assert_eq!(state.coins(0), 2);
        assert_eq!(state.influence(0), 1);
    }

    #[test]
    fn failed_challenge_resolves_the_action() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Duke, Captain], vec![Contessa, Assassin]],
            vec![2, 2],
        );
        game.apply(&mut state, P0, Tax);
        game.apply(&mut state, P1, Challenge);
        game.apply(&mut state, P1, Lose(0));
        assert_eq!(state.influence(1), 1);
        assert_eq!(state.influence(0), 2);
        assert_eq!(state.coins(0), 5);
    }

    #[test]
    fn assassinate_challenge_can_cost_the_game() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Assassin, Captain], vec![Contessa, Duke]],
            vec![3, 2],
        );
        game.apply(&mut state, P0, Action::Assassinate(1));
        game.apply(&mut state, P1, Challenge);
        game.apply(&mut state, P1, Lose(0));
        game.apply(&mut state, P1, Lose(0));
        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, P0), 1.0);
        assert_eq!(game.reward(&state, P1), -1.0);
    }

    #[test]
    fn steal_blocked_by_ambassador_takes_nothing() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Captain, Duke], vec![Ambassador, Assassin]],
            vec![2, 5],
        );
        game.apply(&mut state, P0, Steal(1));
        game.apply(&mut state, P1, Block(Ambassador));
        game.apply(&mut state, P0, Pass);
        assert_eq!(state.coins(0), 2);
        assert_eq!(state.coins(1), 5);
    }

    #[test]
    fn exchange_keeps_hand_and_deck_size() {
        let game = Coup::default();
        let mut state = rigged(
            vec![vec![Ambassador, Captain], vec![Duke, Contessa]],
            vec![2, 2],
        );
        let deck_before = game.view(&state, None).deck_size;
        game.apply(&mut state, P0, Exchange);
        game.apply(&mut state, P1, Pass);
        game.apply(&mut state, P0, Action::Return(0));
        game.apply(&mut state, P0, Action::Return(0));
        assert_eq!(state.influence(0), 2);
        assert_eq!(game.view(&state, None).deck_size, deck_before);
        assert_eq!(card_counts(&state), [3; 5]);
    }

    #[test]
    fn three_player_tax_asks_both_opponents_in_turn_order() {
        let game = Coup::new(3);
        let mut state = rigged(
            vec![
                vec![Duke, Captain],
                vec![Contessa, Assassin],
                vec![Ambassador, Duke],
            ],
            vec![2, 2, 2],
        );
        game.apply(&mut state, P0, Tax);
        // Seat 1 is asked first, then seat 2, then the tax resolves.
        assert_eq!(game.active_players(&state).iter().next(), Some(P1));
        game.apply(&mut state, P1, Pass);
        assert_eq!(game.active_players(&state).iter().next(), Some(P2));
        game.apply(&mut state, P2, Pass);
        assert_eq!(state.coins(0), 5);
        assert_eq!(state.current(), 1);
    }

    #[test]
    fn three_player_only_the_target_may_block_a_steal() {
        let game = Coup::new(3);
        let mut state = rigged(
            vec![
                vec![Captain, Duke],
                vec![Contessa, Assassin],
                vec![Captain, Duke],
            ],
            vec![2, 5, 2],
        );
        game.apply(&mut state, P0, Steal(2)); // targets seat 2
        // Seat 1 is a non-target responder: may pass/challenge but not block.
        let seat1 = game.legal_actions(&state, P1);
        assert!(!seat1.iter().any(|a| matches!(a, Block(_))));
        game.apply(&mut state, P1, Pass);
        // Seat 2 is the target and may block with Captain or Ambassador.
        let seat2 = game.legal_actions(&state, P2);
        assert!(seat2.contains(&Block(Captain)));
    }

    #[test]
    fn eliminated_seats_are_skipped_but_num_players_is_fixed() {
        let game = Coup::new(3);
        // Seat 1 already eliminated (no influence).
        let mut state = rigged(
            vec![vec![Duke, Captain], vec![], vec![Contessa, Assassin]],
            vec![2, 0, 2],
        );
        assert_eq!(game.num_players(), 3, "roster stays fixed");
        game.apply(&mut state, P0, Action::Income);
        // Turn skips the eliminated seat 1 and lands on seat 2.
        assert_eq!(state.current(), 2);
        // A Tax by seat 2 only asks the living seat 0.
        game.apply(&mut state, P2, Tax);
        assert_eq!(game.active_players(&state).iter().next(), Some(P0));
    }

    #[test]
    fn random_self_play_terminates_for_two_to_four_players() {
        for seats in 2..=4u8 {
            let game = Coup::new(seats);
            for seed in 0..40 {
                let state = play_random(game, seed);
                assert!(game.is_terminal(&state));
                assert_eq!(count_survivors(&state), 1, "seats {seats} seed {seed}");
                assert_eq!(card_counts(&state), [3; 5]);
            }
        }
    }

    #[test]
    fn self_play_is_deterministic() {
        let game = Coup::new(3);
        assert_eq!(play_random(game, 1), play_random(game, 1));
        assert_eq!(play_random(game, 17), play_random(game, 17));
    }

    fn count_survivors(state: &CoupState) -> usize {
        (0..state.seats() as usize)
            .filter(|&s| state.influence(s) > 0)
            .count()
    }
}
