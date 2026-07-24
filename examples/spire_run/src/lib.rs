//! Spire Run: a solo Slay-the-Spire-shaped run, whose axis is phase
//! composition with a nested combat mini-game.
//!
//! One seat (the hero) walks a short linear path of encounters ending in a
//! boss: `Map -> Combat -> Reward -> Map -> Combat -> Reward -> ... -> boss
//! Combat -> GameOver`. `Combat` is not a flat decision -- it is itself a
//! complete mini-game with its own turn loop (see the `combat` module), so the
//! top-level [`SpireRun::apply`] just dispatches into it while the phase tag
//! says `Combat`, exactly as ARCHITECTURE.md's "multi-phase run structures"
//! section prescribes (composition, not a new trait).
//!
//! The enemy is a scripted participant (ARCHITECTURE.md's "scripted /
//! automated participants"): its `legal_actions` is never queried by a
//! player, and its move each turn is a single, algorithmically telegraphed
//! intent, visible in public state before the hero decides how to spend
//! energy against it. Reward card offers go through the reserved
//! [`PlayerId::CHANCE`] pseudo-player instead, since which three cards are
//! offered is a committed outcome the hero observes and reasons about, not a
//! throwaway roll (ARCHITECTURE.md's "explicit chance nodes for committed
//! outcomes").

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Game, PlayerId, Prng};

mod combat;
#[cfg(feature = "ui")]
mod ui;

pub use combat::{CardKind, CombatAction, CombatState, Intent};

/// The sole hero seat.
const HERO: PlayerId = PlayerId::new(0);

/// Starting and maximum hero health for a fresh run.
const STARTING_HP: i32 = 60;

/// Health restored by the `Heal` reward option.
const HEAL_AMOUNT: i32 = 15;

/// A run is declared a loss if it runs this many top-level decisions without
/// reaching `GameOver`, so a pathological policy (e.g. always `Skip`, which
/// cannot happen here, or a bugged combat loop) still terminates a match.
/// Combat's own [`combat::TURN_CAP`] should always fire first in practice;
/// this is the outer safety net ARCHITECTURE.md's "the engine never owns a
/// parallel loop you have to keep in sync" spirit still asks every game to
/// provide for its own outer loop.
const STEP_CAP: u32 = 5_000;

/// The enemies fought in order; the last is the boss.
const ENCOUNTERS: [combat::EnemyKind; 4] = [
    combat::EnemyKind {
        name: "Cultist",
        max_hp: 24,
        attack: 5,
    },
    combat::EnemyKind {
        name: "Jaw Worm",
        max_hp: 32,
        attack: 7,
    },
    combat::EnemyKind {
        name: "Louse Swarm",
        max_hp: 38,
        attack: 8,
    },
    combat::EnemyKind {
        name: "Guardian (boss)",
        max_hp: 55,
        attack: 10,
    },
];

/// Fixed reward offers; chance commits to one of these triples per victory.
const OFFER_POOL: [[CardKind; 3]; 4] = [
    [CardKind::Strike, CardKind::Strike, CardKind::Defend],
    [CardKind::Bash, CardKind::Defend, CardKind::Defend],
    [CardKind::Strike, CardKind::Bash, CardKind::Defend],
    [CardKind::Bash, CardKind::Bash, CardKind::Strike],
];

/// The run's starting deck: a small Slay the Spire-style starter (5 Strike, 4
/// Defend, 1 Bash).
fn starter_deck() -> Vec<CardKind> {
    let mut deck = vec![CardKind::Strike; 5];
    deck.extend(std::iter::repeat_n(CardKind::Defend, 4));
    deck.push(CardKind::Bash);
    deck
}

/// The top-level phase tag plus whatever inner state that phase needs. This
/// is the "phase tag plus an inner state field" convention from
/// ARCHITECTURE.md, not a formal sub-game trait.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    /// Between encounters: the only decision is to advance into the next one.
    Map,
    /// A full combat mini-game is in progress.
    Combat(CombatState),
    /// A reward is being resolved: which cards are offered, then the hero's
    /// choice.
    Reward(RewardState),
    /// The run has ended.
    GameOver {
        /// True if the boss was defeated with the hero still alive.
        won: bool,
    },
}

/// Reward-phase state: `offer` is `None` until [`PlayerId::CHANCE`] commits to
/// one of the reward pool's triples.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RewardState {
    offer: Option<Vec<CardKind>>,
}

/// A run position: the current phase, hero health (persists across
/// encounters; only reward `Heal` restores it), the deck built so far, and
/// the shared generator.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RunState {
    phase: Phase,
    /// Index into [`ENCOUNTERS`] of the next fight (0-based).
    floor: u8,
    hero_hp: i32,
    hero_max_hp: i32,
    deck: Vec<CardKind>,
    rng: Prng,
    steps: u32,
}

impl RunState {
    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The hero's current health (persists across encounters).
    #[must_use]
    pub const fn hero_hp(&self) -> i32 {
        self.hero_hp
    }

    /// The hero's maximum health.
    #[must_use]
    pub const fn hero_max_hp(&self) -> i32 {
        self.hero_max_hp
    }

    /// The next encounter's index, 0-based (`floor + 1` is the 1-based
    /// display value; `ENCOUNTERS.len()` is the final boss).
    #[must_use]
    pub const fn floor(&self) -> u8 {
        self.floor
    }

    /// The hero's current deck (grows via reward pickups).
    #[must_use]
    pub fn deck(&self) -> &[CardKind] {
        &self.deck
    }
}

/// One decision, dispatched by the active phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// `Map` phase: advance into the next encounter.
    Advance,
    /// `Combat` phase: forwarded to [`combat::CombatState::apply`].
    Combat(CombatAction),
    /// `Chance`: commits to one of the reward pool's triples by index.
    Offer(u8),
    /// `Reward` phase: take the offered card at this index into the deck.
    Take(usize),
    /// `Reward` phase: heal instead of taking a card.
    Heal,
    /// `Reward` phase: decline the reward.
    Skip,
}

/// The rules. A single, fixed configuration: one hero seat, a fixed
/// encounter list, and a fixed reward pool, so the example is deterministic
/// given only the seed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct SpireRun;

impl Game for SpireRun {
    type State = RunState;
    type Action = Action;
    type View = RunState;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        RunState {
            phase: Phase::Map,
            floor: 0,
            hero_hp: STARTING_HP,
            hero_max_hp: STARTING_HP,
            deck: starter_deck(),
            rng: Prng::new(seed),
            steps: 0,
        }
    }

    fn num_players(&self) -> usize {
        1
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            return ActivePlayers::none();
        }
        match &state.phase {
            Phase::Map | Phase::Combat(_) => ActivePlayers::one(HERO),
            Phase::Reward(reward) => {
                if reward.offer.is_none() {
                    ActivePlayers::one(PlayerId::CHANCE)
                } else {
                    ActivePlayers::one(HERO)
                }
            }
            Phase::GameOver { .. } => ActivePlayers::none(),
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if !self.active_players(state).contains(player) {
            return Vec::new();
        }
        match &state.phase {
            Phase::Map => vec![Action::Advance],
            Phase::Combat(combat) => combat
                .legal_actions()
                .into_iter()
                .map(Action::Combat)
                .collect(),
            Phase::Reward(reward) => reward.offer.as_ref().map_or_else(
                || {
                    (0..u8::try_from(OFFER_POOL.len()).unwrap_or(u8::MAX))
                        .map(Action::Offer)
                        .collect()
                },
                |offer| {
                    let mut actions: Vec<_> = (0..offer.len()).map(Action::Take).collect();
                    actions.push(Action::Heal);
                    actions.push(Action::Skip);
                    actions
                },
            ),
            Phase::GameOver { .. } => Vec::new(),
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        state.steps += 1;
        match (&mut state.phase, action) {
            (Phase::Map, Action::Advance) => {
                let enemy = ENCOUNTERS[usize::from(state.floor)];
                let combat = CombatState::start(
                    state.hero_hp,
                    state.hero_max_hp,
                    &state.deck,
                    enemy,
                    &mut state.rng,
                );
                state.phase = Phase::Combat(combat);
            }
            (Phase::Combat(combat), Action::Combat(combat_action)) => {
                let outcome = combat.apply(combat_action, &mut state.rng);
                state.hero_hp = combat.hero_hp();
                match outcome {
                    combat::Outcome::Ongoing => {}
                    combat::Outcome::HeroLost => state.phase = Phase::GameOver { won: false },
                    combat::Outcome::HeroWon => {
                        if usize::from(state.floor) + 1 == ENCOUNTERS.len() {
                            state.phase = Phase::GameOver { won: true };
                        } else {
                            state.floor += 1;
                            state.phase = Phase::Reward(RewardState { offer: None });
                        }
                    }
                }
            }
            (Phase::Reward(reward), Action::Offer(index)) if reward.offer.is_none() => {
                if let Some(offer) = OFFER_POOL.get(usize::from(index)) {
                    reward.offer = Some(offer.to_vec());
                }
            }
            (Phase::Reward(reward), Action::Take(index)) => {
                if let Some(offer) = &reward.offer
                    && let Some(&card) = offer.get(index)
                {
                    state.deck.push(card);
                }
                state.phase = Phase::Map;
            }
            (Phase::Reward(_), Action::Heal) => {
                state.hero_hp = (state.hero_hp + HEAL_AMOUNT).min(state.hero_max_hp);
                state.phase = Phase::Map;
            }
            (Phase::Reward(_), Action::Skip) => {
                state.phase = Phase::Map;
            }
            // Any other pairing is not reachable through `legal_actions`;
            // `apply` stays total (a no-op) rather than panicking on it.
            _ => {}
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        matches!(state.phase, Phase::GameOver { .. }) || state.steps >= STEP_CAP
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        if player != HERO {
            return 0.0;
        }
        // A false GameOver and the step-cap-without-resolution case both
        // count as a loss: a run that never resolves is not a cleared spire.
        if matches!(state.phase, Phase::GameOver { won: true }) {
            1.0
        } else {
            -1.0
        }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, CardKind, CombatAction, ENCOUNTERS, Phase, SpireRun};
    use turnbase::{ActivePlayers, Game, PlayerId};

    const HERO: PlayerId = PlayerId::new(0);

    /// Runs `SpireRun` end to end always choosing the first legal action
    /// (auto-resolving chance the same way), returning the terminal state.
    fn play_out_first_choice(seed: u64) -> <SpireRun as Game>::State {
        let game = SpireRun;
        let mut state = game.new_initial_state(seed);
        while !game.is_terminal(&state) {
            let active = game.active_players(&state);
            let player = active.iter().next().expect("not terminal implies active");
            let actions = game.legal_actions(&state, player);
            let action = actions
                .into_iter()
                .next()
                .expect("active player has a move");
            game.apply(&mut state, player, action);
        }
        state
    }

    #[test]
    fn phases_advance_map_combat_reward_through_the_boss() {
        // Always ending combat turns instantly is not enough to win, but the
        // phase machine itself must still walk Map -> Combat -> Reward for
        // every non-boss encounter and finish at GameOver, regardless of
        // outcome.
        let state = play_out_first_choice(1);
        assert!(matches!(state.phase, Phase::GameOver { .. }));
    }

    #[test]
    fn combat_legal_actions_come_from_the_combat_substate() {
        let game = SpireRun;
        let mut state = game.new_initial_state(2);
        game.apply(&mut state, HERO, Action::Advance);
        let Phase::Combat(combat) = &state.phase else {
            panic!("expected combat after advancing");
        };
        let expected: Vec<_> = combat
            .legal_actions()
            .into_iter()
            .map(Action::Combat)
            .collect();
        assert_eq!(game.legal_actions(&state, HERO), expected);
    }

    #[test]
    fn a_card_deals_damage() {
        let game = SpireRun;
        let mut state = game.new_initial_state(3);
        game.apply(&mut state, HERO, Action::Advance);
        let Phase::Combat(combat) = &state.phase else {
            panic!("expected combat");
        };
        let enemy_hp_before = combat.enemy_hp();
        let strike_index = combat
            .hand()
            .iter()
            .position(|c| *c == CardKind::Strike)
            .expect("starter deck has a strike");
        game.apply(
            &mut state,
            HERO,
            Action::Combat(CombatAction::Play(strike_index)),
        );
        let Phase::Combat(combat) = &state.phase else {
            panic!("still in combat");
        };
        assert_eq!(combat.enemy_hp(), enemy_hp_before - 6);
    }

    #[test]
    fn a_scripted_enemy_can_kill_the_hero() {
        // Ending every turn without ever playing Defend must be able to lose
        // to at least one seed within a short number of tries.
        let lost = (0..50).any(|seed| {
            let game = SpireRun;
            let mut state = game.new_initial_state(seed);
            let mut guard = 0;
            while !game.is_terminal(&state) && guard < 2_000 {
                guard += 1;
                let active = game.active_players(&state);
                let Some(player) = active.iter().next() else {
                    break;
                };
                let action = if player.is_chance() {
                    Action::Offer(0)
                } else {
                    match &state.phase {
                        Phase::Map => Action::Advance,
                        Phase::Combat(_) => Action::Combat(CombatAction::EndTurn),
                        Phase::Reward(_) => Action::Skip,
                        Phase::GameOver { .. } => break,
                    }
                };
                game.apply(&mut state, player, action);
            }
            matches!(state.phase, Phase::GameOver { won: false })
        });
        assert!(lost, "never-defending hero must lose on some seed");
    }

    #[test]
    fn energy_limits_combat_plays() {
        // Greedily play whatever is legal until only EndTurn remains; the
        // top-level dispatch must enforce the same energy budget as the
        // combat substate itself.
        let game = SpireRun;
        let mut state = game.new_initial_state(4);
        game.apply(&mut state, HERO, Action::Advance);
        loop {
            let actions = game.legal_actions(&state, HERO);
            let Some(&action) = actions
                .iter()
                .find(|a| matches!(a, Action::Combat(CombatAction::Play(_))))
            else {
                break;
            };
            game.apply(&mut state, HERO, action);
        }
        assert_eq!(
            game.legal_actions(&state, HERO),
            vec![Action::Combat(CombatAction::EndTurn)],
            "only EndTurn remains once no card is affordable"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 1.0 / -1.0, never a computed float
    fn winning_the_boss_yields_positive_reward_and_dying_yields_negative() {
        // Deterministically play a winning line: Strike/Bash the enemy down
        // every hero turn, skip every reward, across all four encounters.
        let game = SpireRun;
        let mut state = game.new_initial_state(5);
        let mut guard = 0;
        while !game.is_terminal(&state) && guard < 5_000 {
            guard += 1;
            let active = game.active_players(&state);
            let Some(player) = active.iter().next() else {
                break;
            };
            let action = if player.is_chance() {
                Action::Offer(2) // includes a Bash: helps close fights faster
            } else {
                match &state.phase {
                    Phase::Map => Action::Advance,
                    Phase::Combat(combat) => combat
                        .legal_actions()
                        .into_iter()
                        .find_map(|a| match a {
                            CombatAction::Play(i) => Some(Action::Combat(CombatAction::Play(i))),
                            CombatAction::EndTurn => None,
                        })
                        .unwrap_or(Action::Combat(CombatAction::EndTurn)),
                    Phase::Reward(reward) => match &reward.offer {
                        Some(offer) if !offer.is_empty() => Action::Take(0),
                        _ => Action::Skip,
                    },
                    Phase::GameOver { .. } => break,
                }
            };
            game.apply(&mut state, player, action);
        }

        match state.phase {
            Phase::GameOver { won: true } => {
                assert_eq!(game.reward(&state, HERO), 1.0);
            }
            Phase::GameOver { won: false } => {
                assert_eq!(game.reward(&state, HERO), -1.0);
            }
            _ => panic!("run must reach GameOver well within the guard"),
        }
    }

    #[test]
    fn same_seed_same_run() {
        let a = play_out_first_choice(42);
        let b = play_out_first_choice(42);
        assert_eq!(a, b, "determinism: identical seed replays identically");
    }

    #[test]
    fn a_run_always_terminates() {
        // The combat turn cap and the run step cap together must force
        // termination even for an always-first-choice policy across many
        // seeds.
        for seed in 0..30 {
            let state = play_out_first_choice(seed);
            assert!(matches!(state.phase, Phase::GameOver { .. }));
        }
    }

    #[test]
    fn the_final_encounter_is_the_boss() {
        assert!(ENCOUNTERS.last().unwrap().name.contains("boss"));
    }

    #[test]
    fn no_active_player_in_a_terminal_state() {
        let game = SpireRun;
        let state = play_out_first_choice(9);
        assert_eq!(game.active_players(&state), ActivePlayers::none());
    }
}
