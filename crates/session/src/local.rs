//! [`LocalSession`]: an in-memory host that applies requests directly.

use turnbase::{Game, PlayerId, Prng, sample_chance};
use turnbase_protocol::{Request, Response};

use crate::Session;

/// Offset mixed into the match seed for the chance sampler, so committed
/// chance outcomes do not correlate with a game's own in-state generator.
/// Deliberately the same value `turnbase-match` uses, so a game resolved
/// headlessly and one resolved by the interactive loop agree for a given seed.
const CHANCE_SEED_OFFSET: u64 = 0x00C0_FFEE;

/// An in-memory game: owns a [`Game`] and its state, and applies requests to
/// them directly. No file, socket, or other I/O, and no serialization: the
/// typed [`Request`]/[`Response`] pass straight through.
///
/// This is the authority. A [`crate::FileSession`] is a thin wrapper that
/// loads one of these, submits a single request, and saves it back; a
/// long-lived server would hold one across many requests.
///
/// A committed chance node ([`PlayerId::CHANCE`] active) is resolved
/// automatically from `chance`, so `submit` only ever returns control at a
/// player decision or a terminal state, never stuck on a deck deal a client
/// cannot make. `chance` is seeded from the match seed but offset, so it does
/// not share a stream with the game's own in-state generator.
pub struct LocalSession<G: Game> {
    game: G,
    state: G::State,
    version: u64,
    chance: Prng,
}

impl<G: Game> LocalSession<G> {
    /// Starts a session over `game` at `state`, at version 0, with the chance
    /// sampler seeded from `seed`. Any chance node the opening position starts
    /// on (e.g. an initial deal) is resolved immediately.
    #[must_use]
    pub fn new(game: G, state: G::State, seed: u64) -> Self {
        let mut session = Self {
            game,
            state,
            version: 0,
            chance: Prng::new(seed ^ CHANCE_SEED_OFFSET),
        };
        session.resolve_chance();
        session
    }

    /// Resumes a session at a previously saved `state`, `version`, and chance
    /// sampler, e.g. from a [`crate::FileSession`] save file. Does not
    /// re-resolve chance: a saved position is always left at a player decision
    /// or terminal, never mid-chance.
    #[must_use]
    pub const fn resume(game: G, state: G::State, version: u64, chance: Prng) -> Self {
        Self {
            game,
            state,
            version,
            chance,
        }
    }

    /// Returns the rules governing this session.
    #[must_use]
    pub const fn game(&self) -> &G {
        &self.game
    }

    /// Returns the current authoritative state, e.g. to persist or inspect it.
    #[must_use]
    pub const fn state(&self) -> &G::State {
        &self.state
    }

    /// Returns the current version: the number of actions (player and chance)
    /// applied so far.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Consumes the session, returning its parts for persistence.
    #[must_use]
    pub fn into_parts(self) -> (G, G::State, u64, Prng) {
        (self.game, self.state, self.version, self.chance)
    }

    /// Applies committed chance outcomes until a player is owed a decision or
    /// the match ends, mirroring `turnbase-match`'s loop.
    fn resolve_chance(&mut self) {
        while !self.game.is_terminal(&self.state) {
            let Some(player) = self.game.active_players(&self.state).iter().next() else {
                break;
            };
            if !player.is_chance() {
                break;
            }
            let Some(action) = sample_chance(&self.game, &self.state, &mut self.chance) else {
                break;
            };
            self.game.apply(&mut self.state, PlayerId::CHANCE, action);
            self.version += 1;
        }
    }

    /// Returns the requesting seat's view. Never exposes other seats' private
    /// data: [`Game::view`] does the redaction.
    fn query(&self, player: PlayerId) -> Response<G::View> {
        Response::State {
            version: self.version,
            view: self.game.view(&self.state, Some(player)),
        }
    }

    /// Applies one action, bumping the version and resolving any chance node it
    /// exposes.
    ///
    /// Runs the same guards as [`Game::apply_cloned`] (active seat, legal
    /// action) but in place, turning a rejection into a [`Response::Error`]
    /// rather than a panic.
    fn act(&mut self, player: PlayerId, action: G::Action) -> Response<G::View> {
        if !self.game.active_players(&self.state).contains(player) {
            return Response::Error(format!("{player} is not active"));
        }
        if !self.game.is_legal(&self.state, player, &action) {
            return Response::Error(format!("illegal action for {player}"));
        }
        self.game.apply(&mut self.state, player, action);
        self.version += 1;
        self.resolve_chance();
        Response::Ack
    }
}

impl<G: Game> Session<G> for LocalSession<G> {
    fn submit(&mut self, player: PlayerId, request: Request<G::Action>) -> Response<G::View> {
        match request {
            Request::Query => self.query(player),
            Request::Act(action) => self.act(player, action),
        }
    }
}

#[cfg(test)]
mod tests {
    use turnbase::{ActivePlayers, Game, PlayerId, PlayerView, State};
    use turnbase_protocol::{Request, Response};

    use super::LocalSession;
    use crate::Session;

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// A two-seat game with a per-seat secret, to exercise the port's redaction
    /// promise: a `Query` returns only the requesting seat's private data.
    struct Secret;

    impl Game for Secret {
        type State = State<u32, u32>;
        type Action = u32;
        type View = PlayerView<u32, u32>;

        fn new_initial_state(&self, seed: u64) -> Self::State {
            let mut state = State::new(0, seed);
            state.insert_private(P0, 111);
            state.insert_private(P1, 222);
            state
        }
        fn num_players(&self) -> usize {
            2
        }
        fn active_players(&self, _state: &Self::State) -> ActivePlayers {
            ActivePlayers::one(P0)
        }
        fn legal_actions(&self, _state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
            vec![0]
        }
        fn apply(&self, _state: &mut Self::State, _player: PlayerId, _action: Self::Action) {}
        fn is_terminal(&self, _state: &Self::State) -> bool {
            false
        }
        fn reward(&self, _state: &Self::State, _player: PlayerId) -> f64 {
            0.0
        }
        fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
            state.view_for(viewer)
        }
    }

    #[test]
    fn query_redacts_to_the_requesting_seat() {
        let mut session = LocalSession::new(Secret, Secret.new_initial_state(1), 1);

        let Response::State { view, .. } = session.submit(P0, Request::Query) else {
            panic!("expected a state response");
        };
        assert_eq!(view.own_private, Some(111), "seat 0 sees its own secret");

        let Response::State { view, .. } = session.submit(P1, Request::Query) else {
            panic!("expected a state response");
        };
        assert_eq!(
            view.own_private,
            Some(222),
            "seat 1 sees its own, never seat 0's"
        );
    }

    /// A pure-chance game: `CHANCE` reveals a value 0/1/2, then it is terminal.
    struct RevealOnce;

    impl Game for RevealOnce {
        type State = Option<u8>;
        type Action = u8;
        type View = Option<u8>;

        fn new_initial_state(&self, _seed: u64) -> Self::State {
            None
        }
        fn num_players(&self) -> usize {
            0
        }
        fn active_players(&self, state: &Self::State) -> ActivePlayers {
            if state.is_some() {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::CHANCE)
            }
        }
        fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
            if player.is_chance() && state.is_none() {
                vec![0, 1, 2]
            } else {
                Vec::new()
            }
        }
        fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
            *state = Some(action);
        }
        fn is_terminal(&self, state: &Self::State) -> bool {
            state.is_some()
        }
        fn reward(&self, _state: &Self::State, _player: PlayerId) -> f64 {
            0.0
        }
        fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
            *state
        }
    }

    #[test]
    fn new_resolves_an_opening_chance_node() {
        // The opening position is a chance node; `new` resolves it, so the
        // session lands on a terminal state with a revealed value at version 1.
        let session = LocalSession::new(RevealOnce, RevealOnce.new_initial_state(0), 5);
        assert!(matches!(session.state(), Some(0..=2)));
        assert_eq!(session.version(), 1, "one chance outcome was applied");
    }
}
