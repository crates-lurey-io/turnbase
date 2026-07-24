//! Woodland: two asymmetric factions contest a five-clearing ring.
//!
//! THE AXIS of this example is the "enum-of-enums" convention from
//! `ARCHITECTURE.md`'s "Faction asymmetry" section: [`Action`] is a flat enum
//! with one variant per faction, each wrapping that faction's own,
//! independently-shaped action enum ([`MarquiseAction`], [`AllianceAction`]).
//! `legal_actions`/`apply` dispatch on [`WoodlandState::current`] to the
//! matching faction's own rule system and wrap the result -- ordinary Rust
//! enum composition, not new trait machinery.
//!
//! The Marquise (a builder) marches warriors around the ring, builds
//! structures, and recruits more warriors into built clearings; it scores
//! victory points by building. The Woodland Alliance (a spreader) moves
//! warriors, spreads sympathy tokens, and organizes more warriors into
//! sympathetic clearings; it scores victory points by spreading. Neither
//! faction can take the other's actions -- there is no shared "move" concept,
//! only two disjoint rule systems glued together by [`Action`].
//!
//! Turns strictly alternate (`active_players` is always a singleton), but
//! each turn is a short sequence of decisions: a faction gets up to
//! [`ACTIONS_PER_TURN`] actions (tracked by [`WoodlandState::actions_left`])
//! before control passes, or it may end its turn early. This exercises the
//! "a turn can span many `apply` calls" shape from `ARCHITECTURE.md`'s
//! "Action spaces" section, the same way Risk's phases do, just without
//! phases -- one faction, one flat action set, a budget instead of a phase
//! tag.
//!
//! First faction to reach [`VP_TARGET`] wins outright; a [`TURN_CAP`] ends an
//! otherwise-unresolved match, scored by victory points (a tie is a draw).
//! Because the factions score by unrelated means (buildings vs. sympathy),
//! whichever one reaches the target first via its own path wins -- the
//! asymmetry is real, not cosmetic. Everything is deterministic; the embedded
//! [`Prng`] is carried for consistency with the rest of the workspace even
//! though this game adds no variable setup to seed from it.

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Game, PlayerId, Prng};

#[cfg(feature = "ui")]
mod ui;

/// Number of clearings on the map.
pub const CLEARINGS: usize = 5;

/// Victory points needed to win outright.
pub const VP_TARGET: u32 = 10;

/// Actions a faction may take before its turn passes automatically.
pub const ACTIONS_PER_TURN: u8 = 3;

/// Half-turns (one faction's turn each) after which an unresolved match ends
/// by score.
pub const TURN_CAP: u32 = 48;

/// Victory points scored per building/sympathy token. Five clearings times
/// this value equals [`VP_TARGET`], so fully covering the map is exactly a
/// win.
const VP_PER_MARKER: u32 = 2;

/// Adjacency list for the five-clearing ring: `ADJACENCY[c]` are the
/// clearings bordering `c`.
const ADJACENCY: [&[u8]; CLEARINGS] = [&[4, 1], &[0, 2], &[1, 3], &[2, 4], &[3, 0]];

/// Which side of the board a seat plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Faction {
    /// The builder: scores victory points by constructing buildings.
    Marquise,
    /// The spreader: scores victory points by placing sympathy tokens.
    Alliance,
}

impl Faction {
    /// The seat index this faction plays: Marquise is seat 0, Alliance seat 1.
    #[must_use]
    pub const fn seat(self) -> u32 {
        match self {
            Self::Marquise => 0,
            Self::Alliance => 1,
        }
    }

    const fn other(self) -> Self {
        match self {
            Self::Marquise => Self::Alliance,
            Self::Alliance => Self::Marquise,
        }
    }
}

/// The Marquise's own action enumeration: march warriors, build, recruit, or
/// end the turn early.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MarquiseAction {
    /// Move one warrior from the first (owned, occupied) clearing to the
    /// adjacent second.
    March(u8, u8),
    /// Build in a clearing holding at least one warrior and no building yet.
    Build(u8),
    /// Recruit one warrior into a clearing that already has a building.
    Recruit(u8),
    /// End the turn without using the remaining actions.
    EndTurn,
}

/// The Woodland Alliance's own action enumeration: spread sympathy,
/// organize, move warriors, or end the turn early.
///
/// Deliberately a different shape from [`MarquiseAction`]: the Alliance never
/// builds and the Marquise never spreads sympathy.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AllianceAction {
    /// Place a sympathy token in a clearing holding at least one warrior and
    /// no token yet.
    Spread(u8),
    /// Recruit one warrior into a clearing that already has a sympathy token.
    Organize(u8),
    /// Move one warrior from the first (owned, occupied) clearing to the
    /// adjacent second.
    Move(u8, u8),
    /// End the turn without using the remaining actions.
    EndTurn,
}

/// A move: THE AXIS. A flat wrapper around whichever faction's own action
/// enum applies. See the module docs and `ARCHITECTURE.md`'s "Faction
/// asymmetry" section.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// A Marquise decision.
    Marquise(MarquiseAction),
    /// A Woodland Alliance decision.
    Alliance(AllianceAction),
}

/// A Woodland position: warrior counts, buildings, sympathy tokens, whose
/// turn it is, and how many actions remain in it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WoodlandState {
    warriors_marquise: [u32; CLEARINGS],
    warriors_alliance: [u32; CLEARINGS],
    buildings: [bool; CLEARINGS],
    sympathy: [bool; CLEARINGS],
    vp_marquise: u32,
    vp_alliance: u32,
    current: Faction,
    actions_left: u8,
    turn: u32,
    rng: Prng,
}

impl WoodlandState {
    /// Marquise warriors in `clearing`.
    #[must_use]
    pub const fn warriors_marquise(&self, clearing: usize) -> u32 {
        self.warriors_marquise[clearing]
    }

    /// Alliance warriors in `clearing`.
    #[must_use]
    pub const fn warriors_alliance(&self, clearing: usize) -> u32 {
        self.warriors_alliance[clearing]
    }

    /// Whether `clearing` has a Marquise building.
    #[must_use]
    pub const fn building(&self, clearing: usize) -> bool {
        self.buildings[clearing]
    }

    /// Whether `clearing` has an Alliance sympathy token.
    #[must_use]
    pub const fn sympathy(&self, clearing: usize) -> bool {
        self.sympathy[clearing]
    }

    /// The Marquise's current victory points.
    #[must_use]
    pub const fn vp_marquise(&self) -> u32 {
        self.vp_marquise
    }

    /// The Alliance's current victory points.
    #[must_use]
    pub const fn vp_alliance(&self) -> u32 {
        self.vp_alliance
    }

    /// The faction whose turn is in progress.
    #[must_use]
    pub const fn current(&self) -> Faction {
        self.current
    }

    /// Actions left in the current turn before it passes automatically.
    #[must_use]
    pub const fn actions_left(&self) -> u8 {
        self.actions_left
    }

    /// Half-turns elapsed so far.
    #[must_use]
    pub const fn turn(&self) -> u32 {
        self.turn
    }

    /// Ends the current faction's turn: switches to the other faction and
    /// resets the action budget.
    const fn end_turn(&mut self) {
        self.current = self.current.other();
        self.actions_left = ACTIONS_PER_TURN;
        self.turn += 1;
    }

    /// Spends one action of the current turn's budget, ending the turn
    /// automatically once none remain -- the "actions left this turn"
    /// counter that gives this game its multi-decision turns, mirroring
    /// Risk's phase counters without needing a phase tag.
    const fn spend_action(&mut self) {
        self.actions_left -= 1;
        if self.actions_left == 0 {
            self.end_turn();
        }
    }
}

/// What a player observes. Woodland has no hidden information, so this is a
/// public projection of the whole position.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WoodlandView {
    /// Marquise warriors per clearing.
    pub warriors_marquise: Vec<u32>,
    /// Alliance warriors per clearing.
    pub warriors_alliance: Vec<u32>,
    /// Whether each clearing has a Marquise building.
    pub buildings: Vec<bool>,
    /// Whether each clearing has an Alliance sympathy token.
    pub sympathy: Vec<bool>,
    /// The Marquise's victory points.
    pub vp_marquise: u32,
    /// The Alliance's victory points.
    pub vp_alliance: u32,
    /// The faction to move.
    pub current: Faction,
    /// Actions left in the current turn.
    pub actions_left: u8,
    /// Half-turns elapsed.
    pub turn: u32,
}

/// The rules of Woodland: a fixed two-faction match on the five-clearing
/// ring.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Woodland;

/// The Marquise's legal decisions in the current position.
fn marquise_actions(state: &WoodlandState) -> Vec<MarquiseAction> {
    let mut actions = vec![MarquiseAction::EndTurn];
    for (from, neighbors) in ADJACENCY.iter().enumerate() {
        if state.warriors_marquise[from] == 0 {
            continue;
        }
        let from_id = u8::try_from(from).unwrap();
        for &to in *neighbors {
            actions.push(MarquiseAction::March(from_id, to));
        }
        if !state.buildings[from] {
            actions.push(MarquiseAction::Build(from_id));
        }
    }
    for clearing in 0..CLEARINGS {
        if state.buildings[clearing] {
            actions.push(MarquiseAction::Recruit(u8::try_from(clearing).unwrap()));
        }
    }
    actions
}

/// The Woodland Alliance's legal decisions in the current position.
fn alliance_actions(state: &WoodlandState) -> Vec<AllianceAction> {
    let mut actions = vec![AllianceAction::EndTurn];
    for (from, neighbors) in ADJACENCY.iter().enumerate() {
        if state.warriors_alliance[from] == 0 {
            continue;
        }
        let from_id = u8::try_from(from).unwrap();
        for &to in *neighbors {
            actions.push(AllianceAction::Move(from_id, to));
        }
        if !state.sympathy[from] {
            actions.push(AllianceAction::Spread(from_id));
        }
    }
    for clearing in 0..CLEARINGS {
        if state.sympathy[clearing] {
            actions.push(AllianceAction::Organize(u8::try_from(clearing).unwrap()));
        }
    }
    actions
}

/// Applies one Marquise decision.
fn apply_marquise(state: &mut WoodlandState, action: MarquiseAction) {
    match action {
        MarquiseAction::March(from, to) => {
            state.warriors_marquise[usize::from(from)] -= 1;
            state.warriors_marquise[usize::from(to)] += 1;
            state.spend_action();
        }
        MarquiseAction::Build(clearing) => {
            state.buildings[usize::from(clearing)] = true;
            state.vp_marquise += VP_PER_MARKER;
            state.spend_action();
        }
        MarquiseAction::Recruit(clearing) => {
            state.warriors_marquise[usize::from(clearing)] += 1;
            state.spend_action();
        }
        MarquiseAction::EndTurn => state.end_turn(),
    }
}

/// Applies one Woodland Alliance decision.
fn apply_alliance(state: &mut WoodlandState, action: AllianceAction) {
    match action {
        AllianceAction::Move(from, to) => {
            state.warriors_alliance[usize::from(from)] -= 1;
            state.warriors_alliance[usize::from(to)] += 1;
            state.spend_action();
        }
        AllianceAction::Spread(clearing) => {
            state.sympathy[usize::from(clearing)] = true;
            state.vp_alliance += VP_PER_MARKER;
            state.spend_action();
        }
        AllianceAction::Organize(clearing) => {
            state.warriors_alliance[usize::from(clearing)] += 1;
            state.spend_action();
        }
        AllianceAction::EndTurn => state.end_turn(),
    }
}

impl Game for Woodland {
    type State = WoodlandState;
    type Action = Action;
    type View = WoodlandView;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        let mut warriors_marquise = [0u32; CLEARINGS];
        let mut warriors_alliance = [0u32; CLEARINGS];
        // Opposite ends of the ring: clearing 0 for the Marquise, the last
        // clearing for the Alliance.
        warriors_marquise[0] = 2;
        warriors_alliance[CLEARINGS - 1] = 2;
        WoodlandState {
            warriors_marquise,
            warriors_alliance,
            buildings: [false; CLEARINGS],
            sympathy: [false; CLEARINGS],
            vp_marquise: 0,
            vp_alliance: 0,
            current: Faction::Marquise,
            actions_left: ACTIONS_PER_TURN,
            turn: 0,
            rng: Prng::new(seed),
        }
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(state.current.seat()))
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) || player.index() != state.current.seat() {
            return Vec::new();
        }
        match state.current {
            Faction::Marquise => marquise_actions(state)
                .into_iter()
                .map(Action::Marquise)
                .collect(),
            Faction::Alliance => alliance_actions(state)
                .into_iter()
                .map(Action::Alliance)
                .collect(),
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        match action {
            Action::Marquise(a) => apply_marquise(state, a),
            Action::Alliance(a) => apply_alliance(state, a),
        }
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.vp_marquise >= VP_TARGET || state.vp_alliance >= VP_TARGET || state.turn >= TURN_CAP
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let winner = if state.vp_marquise >= VP_TARGET && state.vp_alliance < VP_TARGET {
            Some(Faction::Marquise)
        } else if state.vp_alliance >= VP_TARGET && state.vp_marquise < VP_TARGET {
            Some(Faction::Alliance)
        } else {
            match state.vp_marquise.cmp(&state.vp_alliance) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(Faction::Marquise),
                std::cmp::Ordering::Less => Some(Faction::Alliance),
            }
        };

        match winner {
            Some(faction) if faction.seat() == player.index() => 1.0,
            Some(_) => -1.0,
            None => 0.0,
        }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        WoodlandView {
            warriors_marquise: state.warriors_marquise.to_vec(),
            warriors_alliance: state.warriors_alliance.to_vec(),
            buildings: state.buildings.to_vec(),
            sympathy: state.sympathy.to_vec(),
            vp_marquise: state.vp_marquise,
            vp_alliance: state.vp_alliance,
            current: state.current,
            actions_left: state.actions_left,
            turn: state.turn,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // rewards are exactly +1.0 / -1.0 / 0.0

    use super::{
        ACTIONS_PER_TURN, Action, AllianceAction, CLEARINGS, Faction, MarquiseAction, TURN_CAP,
        VP_TARGET, Woodland, WoodlandState,
    };
    use turnbase::{Game, PlayerId, Prng};
    use turnbase_bots::{Bot, RandomBot};

    const MARQUISE: PlayerId = PlayerId::new(0);
    const ALLIANCE: PlayerId = PlayerId::new(1);

    /// A hand-built position for exercising one rule in isolation, bypassing
    /// setup and any faction/turn bookkeeping the test itself doesn't care
    /// about.
    fn rigged(
        warriors_marquise: [u32; CLEARINGS],
        warriors_alliance: [u32; CLEARINGS],
        buildings: [bool; CLEARINGS],
        sympathy: [bool; CLEARINGS],
        current: Faction,
    ) -> WoodlandState {
        WoodlandState {
            warriors_marquise,
            warriors_alliance,
            buildings,
            sympathy,
            vp_marquise: 0,
            vp_alliance: 0,
            current,
            actions_left: ACTIONS_PER_TURN,
            turn: 0,
            rng: Prng::new(0),
        }
    }

    #[test]
    fn legal_actions_only_yield_the_active_factions_own_variant() {
        let game = Woodland;
        let state = game.new_initial_state(1);
        assert_eq!(state.current(), Faction::Marquise);
        let marquise_moves = game.legal_actions(&state, MARQUISE);
        assert!(!marquise_moves.is_empty());
        assert!(
            marquise_moves
                .iter()
                .all(|a| matches!(a, Action::Marquise(_))),
            "the Marquise's decisions must all be Action::Marquise"
        );
        assert!(
            game.legal_actions(&state, ALLIANCE).is_empty(),
            "it is not the Alliance's turn yet"
        );
    }

    #[test]
    fn cross_faction_and_off_turn_actions_are_illegal() {
        let game = Woodland;
        let state = game.new_initial_state(1);
        // An Action::Alliance value exists, but it's the Marquise's turn.
        assert!(!game.is_legal(&state, MARQUISE, &Action::Alliance(AllianceAction::EndTurn)));
        // The Alliance is not active yet, regardless of the action's shape.
        assert!(!game.is_legal(&state, ALLIANCE, &Action::Marquise(MarquiseAction::EndTurn)));
    }

    #[test]
    fn march_moves_one_warrior_between_adjacent_clearings() {
        let game = Woodland;
        let mut state = rigged(
            [2, 0, 0, 0, 0],
            [0; CLEARINGS],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        game.apply(
            &mut state,
            MARQUISE,
            Action::Marquise(MarquiseAction::March(0, 1)),
        );
        assert_eq!(state.warriors_marquise(0), 1);
        assert_eq!(state.warriors_marquise(1), 1);
    }

    #[test]
    fn build_then_recruit_scores_and_grows_the_marquise() {
        let game = Woodland;
        let mut state = rigged(
            [1, 0, 0, 0, 0],
            [0; CLEARINGS],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        game.apply(
            &mut state,
            MARQUISE,
            Action::Marquise(MarquiseAction::Build(0)),
        );
        assert!(state.building(0), "clearing 0 now has a building");
        assert_eq!(state.vp_marquise(), 2, "a build scores victory points");

        game.apply(
            &mut state,
            MARQUISE,
            Action::Marquise(MarquiseAction::Recruit(0)),
        );
        assert_eq!(
            state.warriors_marquise(0),
            2,
            "recruiting adds a warrior to the built clearing"
        );
    }

    #[test]
    fn spread_then_organize_scores_and_grows_the_alliance() {
        let game = Woodland;
        let mut state = rigged(
            [0; CLEARINGS],
            [1, 0, 0, 0, 0],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Alliance,
        );
        game.apply(
            &mut state,
            ALLIANCE,
            Action::Alliance(AllianceAction::Spread(0)),
        );
        assert!(state.sympathy(0), "clearing 0 now has a sympathy token");
        assert_eq!(state.vp_alliance(), 2, "a spread scores victory points");

        game.apply(
            &mut state,
            ALLIANCE,
            Action::Alliance(AllianceAction::Organize(0)),
        );
        assert_eq!(
            state.warriors_alliance(0),
            2,
            "organizing adds a warrior to the sympathetic clearing"
        );
    }

    #[test]
    fn end_turn_alternates_the_active_faction() {
        let game = Woodland;
        let mut state = game.new_initial_state(1);
        game.apply(
            &mut state,
            MARQUISE,
            Action::Marquise(MarquiseAction::EndTurn),
        );
        assert_eq!(state.current(), Faction::Alliance);
        assert_eq!(state.actions_left(), ACTIONS_PER_TURN);
        assert_eq!(
            game.active_players(&state).iter().next(),
            Some(ALLIANCE),
            "control passed to the Alliance"
        );
    }

    #[test]
    fn the_actions_left_counter_ends_the_turn_automatically() {
        let game = Woodland;
        let mut state = rigged(
            [5, 0, 0, 0, 0],
            [0; CLEARINGS],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        for step in 0..ACTIONS_PER_TURN {
            assert_eq!(
                state.current(),
                Faction::Marquise,
                "still the Marquise's turn at step {step}"
            );
            game.apply(
                &mut state,
                MARQUISE,
                Action::Marquise(MarquiseAction::March(0, 1)),
            );
        }
        assert_eq!(
            state.current(),
            Faction::Alliance,
            "the turn passed once the budget ran out, with no explicit EndTurn"
        );
        assert_eq!(
            state.actions_left(),
            ACTIONS_PER_TURN,
            "the new turn's budget reset"
        );
    }

    #[test]
    fn reaching_the_target_ends_the_match_with_a_winner() {
        let game = Woodland;
        let mut state = rigged(
            [1, 1, 1, 1, 1],
            [0; CLEARINGS],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        // Building every clearing is exactly VP_TARGET (5 clearings * 2 VP).
        for clearing in 0..CLEARINGS {
            assert!(!game.is_terminal(&state));
            game.apply(
                &mut state,
                MARQUISE,
                Action::Marquise(MarquiseAction::Build(u8::try_from(clearing).unwrap())),
            );
        }
        assert!(game.is_terminal(&state));
        assert_eq!(state.vp_marquise(), VP_TARGET);
        assert_eq!(game.reward(&state, MARQUISE), 1.0);
        assert_eq!(game.reward(&state, ALLIANCE), -1.0);
    }

    #[test]
    fn turn_cap_ends_the_match_and_scores_by_victory_points() {
        let game = Woodland;
        let mut state = rigged(
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        state.vp_marquise = 4;
        state.vp_alliance = 2;
        state.turn = TURN_CAP;
        assert!(game.is_terminal(&state), "the turn cap ends the match");
        assert_eq!(
            game.reward(&state, MARQUISE),
            1.0,
            "higher VP wins at the cap"
        );
        assert_eq!(game.reward(&state, ALLIANCE), -1.0);
    }

    #[test]
    fn a_tied_turn_cap_is_a_draw() {
        let game = Woodland;
        let mut state = rigged(
            [1, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [false; CLEARINGS],
            [false; CLEARINGS],
            Faction::Marquise,
        );
        state.vp_marquise = 4;
        state.vp_alliance = 4;
        state.turn = TURN_CAP;
        assert!(game.is_terminal(&state));
        assert_eq!(game.reward(&state, MARQUISE), 0.0);
        assert_eq!(game.reward(&state, ALLIANCE), 0.0);
    }

    #[test]
    fn the_same_seed_produces_the_same_initial_state() {
        let game = Woodland;
        assert_eq!(game.new_initial_state(42), game.new_initial_state(42));
        assert_eq!(game.new_initial_state(7), game.new_initial_state(7));
    }

    #[test]
    fn random_self_play_always_terminates_with_a_valid_outcome() {
        let game = Woodland;
        for seed in 0..20 {
            let mut state = game.new_initial_state(seed);
            let mut marquise_bot = RandomBot::new(seed);
            let mut alliance_bot = RandomBot::new(seed ^ 0xF00D);
            let mut steps = 0;
            while !game.is_terminal(&state) {
                let player = game.active_players(&state).iter().next().unwrap();
                let bot = if player == MARQUISE {
                    &mut marquise_bot
                } else {
                    &mut alliance_bot
                };
                let action = bot
                    .choose(&game, &state, player)
                    .expect("an active faction always has a legal move (EndTurn, at least)");
                game.apply(&mut state, player, action);
                steps += 1;
                assert!(steps < 100_000, "seed {seed} did not terminate");
            }
            let marquise_reward = game.reward(&state, MARQUISE);
            let alliance_reward = game.reward(&state, ALLIANCE);
            assert_eq!(
                marquise_reward, -alliance_reward,
                "a two-player zero-sum outcome"
            );
            assert!(matches!(marquise_reward, 1.0 | -1.0 | 0.0));
        }
    }
}
