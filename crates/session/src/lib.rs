//! The Session port: submit a request against a running game and get a
//! response, whether the authoritative state lives in this process or (later)
//! a remote one.
//!
//! A [`Session`] is a client-side handle, not the host itself. Only some
//! implementations actually hold the authoritative state:
//!
//! - [`LocalSession`] owns the [`turnbase::Game`] and its state directly and
//!   mutates it in process. This process *is* the host for as long as the
//!   value lives. Zero I/O; this is what tests, a long-lived server, and
//!   self-play hold onto.
//! - [`FileSession`] is not itself a [`Session`]: it is a stateless adapter
//!   *around* a per-call [`LocalSession`]. Each call loads a `LocalSession`
//!   from a JSON save file, submits one request, and saves it back. This is
//!   what a headless CLI drives, one process invocation per request.
//!
//! A future `RemoteSession` would hold no authority at all, only a connection
//! to a host reached over some transport. That, and any pluggable `Store`
//! backend beyond the single JSON file [`FileSession`] ships, are deliberately
//! deferred until a concrete consumer exists (see the workspace design notes).
//!
//! Redaction is automatic: a [`Request::Query`] runs [`turnbase::Game::view`]
//! for the requesting seat first, so only that seat's view is ever produced,
//! never the raw state with every seat's hidden information.

mod file;
mod local;

pub use file::{Error, FileSession};
pub use local::LocalSession;

use turnbase::{Game, PlayerId};
use turnbase_protocol::{Request, Response};

/// A running game, reachable in-process or (later) over a transport.
pub trait Session<G: Game> {
    /// Applies `request` on behalf of `player` and returns the response.
    fn submit(&mut self, player: PlayerId, request: Request<G::Action>) -> Response<G::View>;
}
