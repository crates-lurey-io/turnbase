//! Simplified Risk (2-6 players): conquest on a map graph.
//!
//! The first reference game with *spatial* state and a long, multi-phase turn.
//! It stresses the parts of the [`Game`] trait the small games never touch: a
//! territory adjacency graph, a turn that is a sequence of many decisions across
//! three phases (reinforce, attack, fortify), combat resolved from the in-state
//! generator, area-control bonuses for holding a whole continent, and an action
//! space large enough that legality is checked directly (an [`Game::is_legal`]
//! override) rather than by scanning the enumerated set.
//!
//! The map is a ring of three continents of three territories each; each
//! continent is a triangle, and a single bridge links each continent to the
//! next. Combat is one battle round per `Attack`: the attacker rolls up to three
//! dice, the defender up to two, high dice are paired, and the loser of each
//! pair removes an army; emptying a territory conquers it. Everything is
//! deterministic given the seed, so matches snapshot and replay.

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Game, PlayerId, Prng};

#[cfg(feature = "printable")]
mod ui;

/// Number of territories on the map.
pub const TERRITORIES: usize = 9;

/// Armies placed on every territory at setup.
const INITIAL_ARMIES: u32 = 3;

/// Short territory names, indexed by territory id.
pub const NAMES: [&str; TERRITORIES] = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
];

/// Adjacency list: `ADJACENCY[t]` are the territories bordering `t`. Three
/// triangular continents (0-1-2, 3-4-5, 6-7-8) linked in a ring by the bridges
/// 2-3, 5-6, and 8-0.
const ADJACENCY: [&[u8]; TERRITORIES] = [
    &[1, 2, 8], // 0 Alpha
    &[0, 2],    // 1 Bravo
    &[0, 1, 3], // 2 Charlie
    &[2, 4, 5], // 3 Delta
    &[3, 5],    // 4 Echo
    &[3, 4, 6], // 5 Foxtrot
    &[5, 7, 8], // 6 Golf
    &[6, 8],    // 7 Hotel
    &[6, 7, 0], // 8 India
];

/// Continents as (member territories, army bonus for holding all of them).
const CONTINENTS: [(&[u8], u8); 3] = [(&[0, 1, 2], 2), (&[3, 4, 5], 2), (&[6, 7, 8], 2)];

/// The phase of the current player's turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    /// Placing reinforcement armies onto owned territories.
    Reinforce,
    /// Declaring attacks, or ending the phase.
    Attack,
    /// One optional army move between owned territories, then the turn ends.
    Fortify,
}

/// A single decision. The legal set is phase-dependent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// Place one reinforcement on this owned territory.
    Place(u8),
    /// Fight one battle round from the first territory into the adjacent enemy
    /// second.
    Attack(u8, u8),
    /// Stop attacking and move to the fortify phase.
    EndAttack,
    /// Move all but one army from the first owned territory to the adjacent
    /// owned second, ending the turn.
    Fortify(u8, u8),
    /// End the turn without fortifying.
    EndTurn,
}

/// A Risk position: who owns each territory, the armies on it, and whose
/// structured turn is in progress.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RiskState {
    owner: [u8; TERRITORIES],
    armies: [u32; TERRITORIES],
    current: u8,
    phase: Phase,
    reinforcements: u8,
    seats: u8,
    rng: Prng,
}

impl RiskState {
    /// The seat owning territory `id`.
    #[must_use]
    pub const fn owner(&self, id: usize) -> u8 {
        self.owner[id]
    }

    /// The army count on territory `id`.
    #[must_use]
    pub const fn armies(&self, id: usize) -> u32 {
        self.armies[id]
    }

    /// The seat whose turn it is.
    #[must_use]
    pub const fn current(&self) -> u8 {
        self.current
    }

    /// The phase of the current turn.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Reinforcement armies still to be placed this turn.
    #[must_use]
    pub const fn reinforcements(&self) -> u8 {
        self.reinforcements
    }

    /// The number of seats in the match.
    #[must_use]
    pub const fn seats(&self) -> u8 {
        self.seats
    }

    /// Territories owned by `seat`.
    fn owned(&self, seat: u8) -> usize {
        (0..TERRITORIES).filter(|&t| self.owner[t] == seat).count()
    }

    /// Whether `seat` still holds any territory.
    fn alive(&self, seat: u8) -> bool {
        self.owner.contains(&seat)
    }

    /// The number of seats still holding territory.
    fn alive_count(&self) -> usize {
        (0..self.seats).filter(|&s| self.alive(s)).count()
    }

    /// Reinforcements `seat` receives at the start of a turn: one per three
    /// territories (minimum three) plus the bonus for each wholly held
    /// continent.
    fn reinforcement_count(&self, seat: u8) -> u8 {
        let owned = self.owned(seat);
        let base = (u8::try_from(owned / 3).unwrap_or(u8::MAX)).max(3);
        let bonus: u8 = CONTINENTS
            .iter()
            .filter(|(members, _)| members.iter().all(|&t| self.owner[t as usize] == seat))
            .map(|&(_, b)| b)
            .sum();
        base + bonus
    }

    /// Resolves one battle round from `from` into `to`, conquering `to` if it
    /// empties. Dice come from the in-state generator.
    fn resolve_attack(&mut self, from: u8, to: u8) {
        let attacker = usize::try_from((self.armies[from as usize] - 1).min(3)).unwrap();
        let defender = usize::try_from(self.armies[to as usize].min(2)).unwrap();
        let attack_roll = roll(&mut self.rng, attacker);
        let defend_roll = roll(&mut self.rng, defender);

        let mut defender_losses = 0u32;
        let mut attacker_losses = 0u32;
        for (a, d) in attack_roll.iter().zip(defend_roll.iter()) {
            if a > d {
                defender_losses += 1;
            } else {
                attacker_losses += 1;
            }
        }
        self.armies[from as usize] -= attacker_losses;
        self.armies[to as usize] -= defender_losses;

        if self.armies[to as usize] == 0 {
            // Conquest happens only when every defending die lost, which means
            // the attacker lost none, so `attacker` armies can always move in.
            let moved = u32::try_from(attacker).unwrap();
            self.armies[from as usize] -= moved;
            self.armies[to as usize] = moved;
            self.owner[to as usize] = self.current;
        }
    }

    /// Advances to the next living seat and opens its reinforce phase.
    fn end_turn(&mut self) {
        let mut next = (self.current + 1) % self.seats;
        while !self.alive(next) {
            next = (next + 1) % self.seats;
        }
        self.current = next;
        self.reinforcements = self.reinforcement_count(next);
        self.phase = Phase::Reinforce;
    }
}

/// What a player observes. Risk is a perfect-information game, so a view is the
/// whole position minus the engine's generator.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RiskView {
    /// Owner of each territory.
    pub owner: Vec<u8>,
    /// Armies on each territory.
    pub armies: Vec<u32>,
    /// The seat to move.
    pub current: u8,
    /// The current phase.
    pub phase: Phase,
    /// Reinforcements left to place.
    pub reinforcements: u8,
}

/// The rules of simplified Risk for a chosen number of seats.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Risk {
    seats: u8,
}

impl Default for Risk {
    fn default() -> Self {
        Self::new(2)
    }
}

impl Risk {
    /// Creates a match for `seats` players (clamped to 2..=6).
    #[must_use]
    pub const fn new(seats: u8) -> Self {
        let seats = if seats < 2 {
            2
        } else if seats > 6 {
            6
        } else {
            seats
        };
        Self { seats }
    }
}

/// Rolls `count` dice and returns them sorted high to low.
fn roll(rng: &mut Prng, count: usize) -> Vec<u8> {
    let mut dice: Vec<u8> = (0..count)
        .map(|_| u8::try_from(rng.range(1, 7)).unwrap())
        .collect();
    dice.sort_unstable_by(|a, b| b.cmp(a));
    dice
}

/// Whether `from` and `to` share a border.
fn adjacent(from: u8, to: u8) -> bool {
    ADJACENCY[from as usize].contains(&to)
}

fn seat_of(player: PlayerId) -> u8 {
    u8::try_from(player.index()).expect("seat fits in u8")
}

impl Game for Risk {
    type State = RiskState;
    type Action = Action;
    type View = RiskView;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let mut rng = Prng::new(seed);
        // Shuffle the territories, then deal them round-robin to the seats.
        let mut order: Vec<u8> = (0..u8::try_from(TERRITORIES).unwrap()).collect();
        rng.shuffle(&mut order);
        let mut owner = [0u8; TERRITORIES];
        for (deal, &territory) in order.iter().enumerate() {
            owner[territory as usize] = u8::try_from(deal % usize::from(self.seats)).unwrap();
        }

        let mut state = RiskState {
            owner,
            armies: [INITIAL_ARMIES; TERRITORIES],
            current: 0,
            phase: Phase::Reinforce,
            reinforcements: 0,
            seats: self.seats,
            rng,
        };
        state.reinforcements = state.reinforcement_count(0);
        state
    }

    fn num_players(&self) -> usize {
        usize::from(self.seats)
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(u32::from(state.current)))
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) || seat_of(player) != state.current {
            return Vec::new();
        }
        let seat = state.current;
        match state.phase {
            Phase::Reinforce => (0..TERRITORIES)
                .filter(|&t| state.owner[t] == seat)
                .map(|t| Action::Place(u8::try_from(t).unwrap()))
                .collect(),
            Phase::Attack => {
                let mut actions = vec![Action::EndAttack];
                for (from, neighbors) in ADJACENCY.iter().enumerate() {
                    if state.owner[from] != seat || state.armies[from] < 2 {
                        continue;
                    }
                    for &to in *neighbors {
                        if state.owner[to as usize] != seat {
                            actions.push(Action::Attack(u8::try_from(from).unwrap(), to));
                        }
                    }
                }
                actions
            }
            Phase::Fortify => {
                let mut actions = vec![Action::EndTurn];
                for (from, neighbors) in ADJACENCY.iter().enumerate() {
                    if state.owner[from] != seat || state.armies[from] < 2 {
                        continue;
                    }
                    for &to in *neighbors {
                        if state.owner[to as usize] == seat {
                            actions.push(Action::Fortify(u8::try_from(from).unwrap(), to));
                        }
                    }
                }
                actions
            }
        }
    }

    /// Direct legality check, overriding the default enumerate-and-scan. Attack
    /// and fortify targets are combinatorial, so checking the rule for one
    /// action is cheaper (and clearer) than building the whole list.
    fn is_legal(&self, state: &Self::State, player: PlayerId, action: &Self::Action) -> bool {
        if self.is_terminal(state) || seat_of(player) != state.current {
            return false;
        }
        let seat = state.current;
        match (state.phase, action) {
            (Phase::Reinforce, Action::Place(t)) => {
                state.reinforcements > 0 && state.owner[*t as usize] == seat
            }
            (Phase::Attack, Action::EndAttack) | (Phase::Fortify, Action::EndTurn) => true,
            (Phase::Attack, Action::Attack(from, to)) => {
                state.owner[*from as usize] == seat
                    && state.armies[*from as usize] >= 2
                    && state.owner[*to as usize] != seat
                    && adjacent(*from, *to)
            }
            (Phase::Fortify, Action::Fortify(from, to)) => {
                from != to
                    && state.owner[*from as usize] == seat
                    && state.owner[*to as usize] == seat
                    && state.armies[*from as usize] >= 2
                    && adjacent(*from, *to)
            }
            _ => false,
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        match action {
            Action::Place(t) => {
                state.armies[t as usize] += 1;
                state.reinforcements -= 1;
                if state.reinforcements == 0 {
                    state.phase = Phase::Attack;
                }
            }
            Action::Attack(from, to) => state.resolve_attack(from, to),
            Action::EndAttack => state.phase = Phase::Fortify,
            Action::Fortify(from, to) => {
                let moved = state.armies[from as usize] - 1;
                state.armies[to as usize] += moved;
                state.armies[from as usize] = 1;
                state.end_turn();
            }
            Action::EndTurn => state.end_turn(),
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.alive_count() <= 1
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        if state.alive(seat_of(player)) && self.is_terminal(state) {
            1.0
        } else {
            -1.0
        }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        RiskView {
            owner: state.owner.to_vec(),
            armies: state.armies.to_vec(),
            current: state.current,
            phase: state.phase,
            reinforcements: state.reinforcements,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // rewards are exactly +1.0 / -1.0

    use super::{Action, Phase, Risk, RiskState, TERRITORIES};
    use turnbase::{Game, PlayerId, Prng};

    const P0: PlayerId = PlayerId::new(0);

    fn assert_invariants(state: &RiskState, seats: u8) {
        for t in 0..TERRITORIES {
            assert!(state.armies(t) >= 1, "territory {t} has no armies");
            assert!(state.owner(t) < seats, "territory {t} owner out of range");
        }
        assert!(state.alive_count() >= 1, "no living seats");
    }

    /// Picks a legal action under an aggressive policy: attack whenever
    /// possible, otherwise advance the phase. Uniform-random play barely
    /// terminates Risk (random players almost never choose to attack), so the
    /// driver commits to attacking, which strictly destroys armies and forces
    /// progress toward a winner while still exercising every rule and phase.
    fn aggressive_action(state: &RiskState, actions: &[Action], rng: &mut Prng) -> Action {
        match state.phase() {
            Phase::Attack => {
                let attacks: Vec<Action> = actions
                    .iter()
                    .copied()
                    .filter(|a| matches!(a, Action::Attack(_, _)))
                    .collect();
                if attacks.is_empty() {
                    Action::EndAttack
                } else {
                    attacks[usize::try_from(rng.below(attacks.len() as u64)).unwrap()]
                }
            }
            Phase::Fortify => Action::EndTurn,
            Phase::Reinforce => actions[usize::try_from(rng.below(actions.len() as u64)).unwrap()],
        }
    }

    /// Plays a full aggressive match, checking invariants at every step.
    fn play_aggressive(game: Risk, seed: u64) -> RiskState {
        let mut state = game.new_initial_state(seed);
        let mut rng = Prng::new(seed ^ 0xF00D);
        let mut steps = 0;
        while !game.is_terminal(&state) {
            assert_invariants(&state, game.seats);
            let player = game.active_players(&state).iter().next().unwrap();
            let actions = game.legal_actions(&state, player);
            assert!(!actions.is_empty(), "an active player must have moves");
            let action = aggressive_action(&state, &actions, &mut rng);
            game.apply(&mut state, player, action);
            steps += 1;
            assert!(steps < 500_000, "seed {seed} did not terminate");
        }
        state
    }

    /// A rigged state: seat 0 to reinforce, with the given ownership and armies.
    fn rigged(owner: [u8; TERRITORIES], armies: [u32; TERRITORIES], seats: u8) -> RiskState {
        RiskState {
            owner,
            armies,
            current: 0,
            phase: Phase::Reinforce,
            reinforcements: 0,
            seats,
            rng: Prng::new(0),
        }
    }

    #[test]
    fn setup_partitions_the_map() {
        let game = Risk::new(3);
        let state = game.new_initial_state(7);
        let mut counts = [0usize; 3];
        for t in 0..TERRITORIES {
            assert_eq!(state.armies(t), 3);
            counts[state.owner(t) as usize] += 1;
        }
        assert_eq!(counts.iter().sum::<usize>(), TERRITORIES);
        // Nine territories dealt round-robin to three seats: three each.
        assert!(counts.iter().all(|&c| c == 3), "uneven deal: {counts:?}");
    }

    #[test]
    fn reinforcements_count_territories_and_continents() {
        // Seat 0 holds continent 0 (0,1,2) plus 3 and 6: base max(3, 5/3) = 3,
        // plus the continent-0 bonus of 2. Owning 6 denies seat 1 continent 2.
        let mut owner = [1u8; TERRITORIES];
        owner[0] = 0;
        owner[1] = 0;
        owner[2] = 0;
        owner[3] = 0;
        owner[6] = 0;
        let state = rigged(owner, [3; TERRITORIES], 2);
        assert_eq!(state.reinforcement_count(0), 5);
        // Seat 1 holds 4, 5, 7, 8: base max(3, 4/3) = 3, no whole continent.
        assert_eq!(state.reinforcement_count(1), 3);
    }

    #[test]
    fn reinforce_phase_places_then_advances() {
        let game = Risk::default();
        let mut state = game.new_initial_state(1);
        let start = state.reinforcements();
        assert!(start >= 3 && matches!(state.phase(), Phase::Reinforce));
        let mine: Vec<u8> = (0..TERRITORIES)
            .filter(|&t| state.owner(t) == 0)
            .map(|t| u8::try_from(t).unwrap())
            .collect();
        for _ in 0..start {
            game.apply(&mut state, P0, Action::Place(mine[0]));
        }
        assert_eq!(state.reinforcements(), 0);
        assert_eq!(state.phase(), Phase::Attack, "placing all opens the attack");
    }

    #[test]
    fn is_legal_agrees_with_the_enumerated_set() {
        // The direct is_legal override must match legal_actions across a real
        // game, and reject a plainly illegal move.
        let game = Risk::new(3);
        for seed in 0..8 {
            let mut state = game.new_initial_state(seed);
            let mut rng = Prng::new(seed);
            // A bounded walk is enough to visit every phase many times.
            for _ in 0..3000 {
                if game.is_terminal(&state) {
                    break;
                }
                let player = game.active_players(&state).iter().next().unwrap();
                let actions = game.legal_actions(&state, player);
                for action in &actions {
                    assert!(game.is_legal(&state, player, action));
                }
                // A move by a non-active seat is never legal.
                let other = PlayerId::new(u32::from((state.current() + 1) % game.seats));
                assert!(!game.is_legal(&state, other, &Action::EndAttack));
                let action = aggressive_action(&state, &actions, &mut rng);
                game.apply(&mut state, player, action);
            }
        }
    }

    #[test]
    fn overwhelming_attack_eventually_conquers() {
        // Seat 0 stacks Alpha(0) and hammers Bravo(1), held by seat 1 with one
        // army. With a large stack the conquest is certain within many rounds.
        let mut owner = [0u8; TERRITORIES];
        owner[1] = 1;
        let mut armies = [1u32; TERRITORIES];
        armies[0] = 20;
        let mut state = rigged(owner, armies, 2);
        state.phase = Phase::Attack;
        let game = Risk::default();

        let mut rounds = 0;
        while state.owner(1) == 1 {
            game.apply(&mut state, P0, Action::Attack(0, 1));
            rounds += 1;
            assert!(rounds < 500, "20 armies failed to take one defender");
        }
        assert_eq!(state.owner(1), 0, "Bravo was conquered");
        assert!(state.armies(1) >= 1, "a conquered territory is occupied");
    }

    #[test]
    fn fortify_moves_all_but_one_and_ends_turn() {
        let mut owner = [1u8; TERRITORIES];
        owner[0] = 0;
        owner[1] = 0; // adjacent to 0
        let mut armies = [1u32; TERRITORIES];
        armies[0] = 5;
        let mut state = rigged(owner, armies, 2);
        state.phase = Phase::Fortify;
        let game = Risk::default();

        game.apply(&mut state, P0, Action::Fortify(0, 1));
        assert_eq!(state.armies(0), 1, "all but one moved out");
        assert_eq!(state.armies(1), 5, "1 + 4 moved in");
        assert_eq!(state.current(), 1, "the turn passed on");
        assert_eq!(state.phase(), Phase::Reinforce);
    }

    #[test]
    fn random_self_play_terminates_with_one_winner() {
        for seats in 2..=4 {
            let game = Risk::new(seats);
            for seed in 0..12 {
                let end = play_aggressive(game, seed);
                assert!(game.is_terminal(&end));
                assert_eq!(end.alive_count(), 1, "exactly one seat should remain");
                let winner = (0..seats).find(|&s| end.alive(s)).unwrap();
                assert_eq!(game.reward(&end, PlayerId::new(u32::from(winner))), 1.0);
                let loser = (winner + 1) % seats;
                assert_eq!(game.reward(&end, PlayerId::new(u32::from(loser))), -1.0);
            }
        }
    }
}
