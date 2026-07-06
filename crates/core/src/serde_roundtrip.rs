//! Snapshot/resume and replay tests for the `serde` feature.
//!
//! These validate the headline consequence of keeping the generator inside
//! `State`: serializing the state serializes the generator position too, so a
//! match resumes from a single snapshot in O(1) and continues with identical
//! rolls, without replaying the action log from turn 1.

use crate::{ActivePlayers, Game, PlayerId, State};

const P0: PlayerId = PlayerId::new(0);

/// A one-seat game that draws 1..=6 from the in-state generator each move, so a
/// state's future depends on its embedded generator position.
struct Roll;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Draw,
}

impl Game for Roll {
    type State = State<u32, u32>;
    type Action = Act;
    type View = u32;

    fn new_initial_state(&self, seed: u64) -> Self::State {
        State::new(0, seed)
    }
    fn num_players(&self) -> usize {
        1
    }
    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(P0)
        }
    }
    fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) {
            Vec::new()
        } else {
            vec![Act::Draw]
        }
    }
    fn apply(&self, state: &mut Self::State, _player: PlayerId, _action: Self::Action) {
        let roll = state.rng_mut().range(1, 7);
        *state.public_mut() += u32::try_from(roll).unwrap();
    }
    fn is_terminal(&self, state: &Self::State) -> bool {
        *state.public() >= 1000
    }
    fn reward(&self, _state: &Self::State, _player: PlayerId) -> f64 {
        0.0
    }
    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        *state.public()
    }
}

fn play(seed: u64, moves: usize) -> State<u32, u32> {
    let game = Roll;
    let mut state = game.new_initial_state(seed);
    for _ in 0..moves {
        game.apply(&mut state, P0, Act::Draw);
    }
    state
}

#[test]
fn prng_position_survives_the_round_trip() {
    let state = play(1, 4);
    let json = serde_json::to_string(&state).unwrap();
    let back: State<u32, u32> = serde_json::from_str(&json).unwrap();
    assert_eq!(state, back);
    assert_eq!(state.rng().position(), back.rng().position());
}

#[test]
fn resumed_state_continues_identically() {
    let game = Roll;
    let mut original = play(42, 3);
    let json = serde_json::to_string(&original).unwrap();
    let mut resumed: State<u32, u32> = serde_json::from_str(&json).unwrap();

    for _ in 0..5 {
        game.apply(&mut original, P0, Act::Draw);
        game.apply(&mut resumed, P0, Act::Draw);
    }
    assert_eq!(
        original, resumed,
        "resumed rolls match, so the RNG was snapshotted"
    );
}

#[test]
fn resume_from_snapshot_equals_straight_through() {
    let game = Roll;
    let straight = play(7, 8);

    // Snapshot after 3 moves, resume from the snapshot alone, play 5 more.
    let snapshot = serde_json::to_string(&play(7, 3)).unwrap();
    let mut resumed: State<u32, u32> = serde_json::from_str(&snapshot).unwrap();
    for _ in 0..5 {
        game.apply(&mut resumed, P0, Act::Draw);
    }
    assert_eq!(resumed, straight, "no replay from turn 1 needed to resume");
}

#[test]
fn replay_from_seed_is_deterministic() {
    assert_eq!(play(99, 20), play(99, 20));
}

#[test]
fn private_zones_survive_the_round_trip() {
    let mut state: State<u32, u32> = State::new(5, 1);
    state.insert_private(PlayerId::new(0), 10);
    state.insert_private(PlayerId::new(1), 20);

    let json = serde_json::to_string(&state).unwrap();
    let back: State<u32, u32> = serde_json::from_str(&json).unwrap();
    assert_eq!(state, back);
    assert_eq!(back.private(PlayerId::new(1)), Some(&20));
}
