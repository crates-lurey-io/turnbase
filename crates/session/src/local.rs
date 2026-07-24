//! [`LocalSession`]: an in-memory host that applies requests directly.

use turnbase::{Game, PlayerId};
use turnbase_protocol::{Request, Response};

use crate::Session;

/// An in-memory game: owns a [`Game`] and its state, and applies requests to
/// them directly. No file, socket, or other I/O, and no serialization: the
/// typed [`Request`]/[`Response`] pass straight through.
///
/// This is the authority. A [`crate::FileSession`] is a thin wrapper that
/// loads one of these, submits a single request, and saves it back; a
/// long-lived server would hold one across many requests.
pub struct LocalSession<G: Game> {
    game: G,
    state: G::State,
    version: u64,
}

impl<G: Game> LocalSession<G> {
    /// Starts a session over `game` at `state`, at version 0.
    #[must_use]
    pub const fn new(game: G, state: G::State) -> Self {
        Self {
            game,
            state,
            version: 0,
        }
    }

    /// Resumes a session at a previously saved `state` and `version`, e.g.
    /// from a [`crate::FileSession`] save file.
    #[must_use]
    pub const fn resume(game: G, state: G::State, version: u64) -> Self {
        Self {
            game,
            state,
            version,
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

    /// Returns the current version: the number of actions applied so far.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Consumes the session, returning its parts for persistence.
    #[must_use]
    pub fn into_parts(self) -> (G, G::State, u64) {
        (self.game, self.state, self.version)
    }

    /// Returns the requesting seat's view. Never exposes other seats' private
    /// data: [`Game::view`] does the redaction.
    fn query(&self, player: PlayerId) -> Response<G::View> {
        Response::State {
            version: self.version,
            view: self.game.view(&self.state, Some(player)),
        }
    }

    /// Applies one action, bumping the version on success.
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
