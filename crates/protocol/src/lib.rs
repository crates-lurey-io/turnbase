//! Request/response wire types shared by every Turnbase client and host.
//!
//! The envelope is typed: [`Request<A>`] carries a game's action and
//! [`Response<V>`] carries a seat's view, rather than opaque bytes. This keeps
//! the in-process and file-backed paths serialization-free and type-safe, and
//! lets a real transport serialize the whole envelope in one clean pass (no
//! bytes-inside-JSON double-encoding).
//!
//! These types are generic over the action and view types, not over
//! `turnbase::Game`, so this crate has no dependency on `turnbase` and one
//! definition serves every game. If a future remote transport ever needs to
//! move messages without knowing the concrete types (a universal host, a
//! polyglot bus), that erasure belongs *inside the transport*, not here: the
//! port stays typed.

use serde::{Deserialize, Serialize};

/// Wire-format version, bumped on any breaking change to a type in this crate.
///
/// Stamped into save files and (eventually) exchanged with a remote host so a
/// mismatch fails fast on an explicit check instead of surfacing as a
/// confusing deserialization error deeper in the stack.
pub const PROTOCOL_VERSION: u32 = 0;

/// A message from a client to whichever process holds the authoritative state.
///
/// `A` is a game's `Action` type. The action is carried by value so a host can
/// move it straight into `Game::apply` without cloning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request<A> {
    /// Ask for the requesting seat's current view, with no side effects.
    Query,
    /// Submit an action to apply.
    Act(A),
}

/// A message back to the client. `V` is a game's `View` type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response<V> {
    /// A seat's current view, plus the state version it was taken at.
    ///
    /// `version` is a counter the host bumps once per applied action. Nothing
    /// consumes it yet; it is banked now so a later `QuerySince(version)` can
    /// answer "nothing changed" or send a delta without a wire-format break,
    /// and so a polling client can tell a stale snapshot from a fresh one.
    State {
        /// The state version this view was taken at.
        version: u64,
        /// The requesting seat's view.
        view: V,
    },
    /// An action was accepted and applied. Deliberately carries no state: a
    /// write is acknowledged cheaply, and a client fetches the result with an
    /// explicit [`Request::Query`] only when it actually needs to render.
    Ack,
    /// The request was rejected. The message is for humans and logs, not for
    /// programmatic matching.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::{Request, Response};

    #[test]
    fn act_request_round_trips_through_json() {
        let request = Request::Act(42u32);
        let json = serde_json::to_string(&request).expect("serialize");
        let decoded: Request<u32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(request, decoded);
    }

    #[test]
    fn query_request_round_trips_through_json() {
        let request: Request<u32> = Request::Query;
        let json = serde_json::to_string(&request).expect("serialize");
        let decoded: Request<u32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(request, decoded);
    }

    #[test]
    fn state_response_encodes_its_view_inline_not_as_bytes() {
        let response = Response::State {
            version: 7,
            view: 99u32,
        };
        let json = serde_json::to_string(&response).expect("serialize");
        // The whole point of the typed envelope: the view is a plain value in
        // the JSON, not a nested byte array.
        assert!(json.contains("\"view\":99"), "got {json}");
        let decoded: Response<u32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(response, decoded);
    }

    #[test]
    fn error_response_round_trips() {
        let response: Response<u32> = Response::Error("no session found".to_owned());
        let json = serde_json::to_string(&response).expect("serialize");
        let decoded: Response<u32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(response, decoded);
    }
}
