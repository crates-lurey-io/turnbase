//! [`FileSession`]: a stateless, file-backed adapter over [`LocalSession`].
//!
//! The only place in this crate that touches disk. Each call loads a
//! [`LocalSession`] from a JSON save file, submits one typed request, and
//! (only if an action was actually applied) saves it back. This is the shape a
//! headless CLI drives: one process invocation per request, with the whole
//! game resumed from a single self-describing file.
//!
//! Only the *state* is serialized (into the save file); the typed request and
//! response pass through unserialized. Actions and views only ever need
//! serializing at a real wire boundary, which is a future transport's job.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use turnbase::{Game, PlayerId};
use turnbase_protocol::{PROTOCOL_VERSION, Request, Response};

use crate::{LocalSession, Session};

/// The on-disk envelope: everything needed to resume a game from one file.
///
/// It stores the [`Game`] value (its per-match configuration, e.g. player
/// count) alongside the state, so a resumed session cannot drift from the
/// config it was created with, and `query`/`act` need no configuration flags
/// of their own. `protocol_version` is stamped so a stale save fails fast on
/// an explicit check (see [`Error::ProtocolMismatch`]).
#[derive(Serialize, Deserialize)]
#[serde(bound(
    serialize = "G: Serialize, G::State: Serialize",
    deserialize = "G: Deserialize<'de>, G::State: Deserialize<'de>"
))]
struct SaveFile<G: Game> {
    protocol_version: u32,
    version: u64,
    game: G,
    state: G::State,
}

/// A stateless, file-backed [`Session`]: create a save, then query or drive it
/// across separate process invocations.
pub struct FileSession;

impl FileSession {
    /// Creates a new save file holding `game` at `state` (version 0) and
    /// returns its resolved path. Never overwrites an existing file.
    ///
    /// `path`'s `None` case generates one under the system temp directory, so
    /// a caller need not name a file up front.
    ///
    /// # Errors
    /// Returns [`Error::AlreadyExists`] if `path` is `Some` and a file is
    /// already there, or [`Error::Io`]/[`Error::Serde`] if writing fails.
    pub fn create<G>(game: G, path: Option<PathBuf>, state: G::State) -> Result<PathBuf, Error>
    where
        G: Game + Serialize,
        G::State: Serialize,
    {
        let path = path.unwrap_or_else(generate_temp_path);
        if path.exists() {
            return Err(Error::AlreadyExists(path));
        }
        write_save(
            &path,
            &SaveFile {
                protocol_version: PROTOCOL_VERSION,
                version: 0,
                game,
                state,
            },
        )?;
        Ok(path)
    }

    /// Loads the save at `path`, applies `request` for `player`, saves back if
    /// an action was applied, and returns the response.
    ///
    /// A [`Request::Query`] and a rejected action leave the file untouched;
    /// only a successful apply ([`Response::Ack`]) is persisted.
    ///
    /// # Errors
    /// Returns [`Error::NotFound`] if `path` does not exist,
    /// [`Error::ProtocolMismatch`] if the save was written by an incompatible
    /// protocol version, [`Error::Serde`] if it does not parse, or
    /// [`Error::Io`] if reading or writing fails.
    pub fn handle<G>(
        path: &Path,
        player: PlayerId,
        request: Request<G::Action>,
    ) -> Result<Response<G::View>, Error>
    where
        G: Game + Serialize + DeserializeOwned,
        G::State: Serialize + DeserializeOwned,
    {
        if !path.exists() {
            return Err(Error::NotFound(path.to_path_buf()));
        }
        let save: SaveFile<G> = read_save(path)?;
        let mut session = LocalSession::resume(save.game, save.state, save.version);
        let response = session.submit(player, request);

        // Only an applied action changes state or version; skip the write (and
        // its serialization cost) on queries and rejected actions.
        if matches!(response, Response::Ack) {
            let (game, state, version) = session.into_parts();
            write_save(
                path,
                &SaveFile {
                    protocol_version: PROTOCOL_VERSION,
                    version,
                    game,
                    state,
                },
            )?;
        }
        Ok(response)
    }
}

/// Errors from the file adapter itself.
///
/// Distinct from [`Response::Error`]: that is the *game* rejecting a request
/// (illegal action); these are the *file* misbehaving (missing, already
/// exists, unreadable, wrong protocol version).
#[derive(Debug)]
pub enum Error {
    /// [`FileSession::create`] was asked to write where a file already exists.
    AlreadyExists(PathBuf),
    /// [`FileSession::handle`] was asked to load a path with no file.
    NotFound(PathBuf),
    /// The save was written by an incompatible [`PROTOCOL_VERSION`].
    ProtocolMismatch {
        /// The version stamped in the save file.
        found: u32,
        /// The version this build understands.
        expected: u32,
    },
    /// Reading or writing the save file failed.
    Io(io::Error),
    /// The save file did not parse, or a state failed to serialize.
    Serde(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(path) => write!(f, "session already exists at {}", path.display()),
            Self::NotFound(path) => write!(f, "no session found at {}", path.display()),
            Self::ProtocolMismatch { found, expected } => write!(
                f,
                "save file protocol version {found} is incompatible with {expected}"
            ),
            Self::Io(err) => write!(f, "session I/O error: {err}"),
            Self::Serde(err) => write!(f, "session data error: {err}"),
        }
    }
}

impl std::error::Error for Error {}

fn generate_temp_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    std::env::temp_dir().join(format!("turnbase-{pid}-{nanos}.json"))
}

fn read_save<G>(path: &Path) -> Result<SaveFile<G>, Error>
where
    G: Game + DeserializeOwned,
    G::State: DeserializeOwned,
{
    let bytes = std::fs::read(path).map_err(Error::Io)?;
    let save: SaveFile<G> = serde_json::from_slice(&bytes).map_err(Error::Serde)?;
    if save.protocol_version != PROTOCOL_VERSION {
        return Err(Error::ProtocolMismatch {
            found: save.protocol_version,
            expected: PROTOCOL_VERSION,
        });
    }
    Ok(save)
}

fn write_save<G>(path: &Path, save: &SaveFile<G>) -> Result<(), Error>
where
    G: Game + Serialize,
    G::State: Serialize,
{
    let bytes = serde_json::to_vec_pretty(save).map_err(Error::Serde)?;
    std::fs::write(path, bytes).map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::{Error, FileSession};
    use serde::{Deserialize, Serialize};
    use turnbase::{ActivePlayers, Game, PlayerId};
    use turnbase_protocol::{Request, Response};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Two seats alternately add 1 to a shared total; whoever reaches 3 wins.
    /// Only the game and its state need to serialize (for the save file); the
    /// action is passed typed, so `Bump` needs no serde derive.
    #[derive(Serialize, Deserialize)]
    struct CountToThree;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Bump;

    impl Game for CountToThree {
        type State = u32;
        type Action = Bump;
        type View = u32;

        fn new_initial_state(&self, _seed: u64) -> Self::State {
            0
        }
        fn num_players(&self) -> usize {
            2
        }
        fn active_players(&self, state: &Self::State) -> ActivePlayers {
            if self.is_terminal(state) {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::new(state % 2))
            }
        }
        fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
            if self.is_terminal(state) {
                Vec::new()
            } else {
                vec![Bump]
            }
        }
        fn apply(&self, state: &mut Self::State, _player: PlayerId, _action: Self::Action) {
            *state += 1;
        }
        fn is_terminal(&self, state: &Self::State) -> bool {
            *state >= 3
        }
        fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
            let winner = (state + 1) % 2;
            if player.index() == winner { 1.0 } else { -1.0 }
        }
        fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
            *state
        }
    }

    fn state_of(response: &Response<u32>) -> (u64, u32) {
        let Response::State { version, view } = response else {
            panic!("expected a State response, got {response:?}");
        };
        (*version, *view)
    }

    #[test]
    fn create_refuses_to_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.json");
        std::fs::write(&path, b"anything").unwrap();

        let result = FileSession::create(CountToThree, Some(path), 0);
        assert!(matches!(result, Err(Error::AlreadyExists(_))));
    }

    #[test]
    fn handle_round_trips_state_and_bumps_version_only_on_apply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.json");
        let path = FileSession::create(CountToThree, Some(path), 0).unwrap();

        // A query is a no-op: version 0, total 0, file unchanged.
        let response = FileSession::handle::<CountToThree>(&path, P0, Request::Query).unwrap();
        assert_eq!(state_of(&response), (0, 0));

        // Applying an action acks and persists the bumped state.
        assert!(matches!(
            FileSession::handle::<CountToThree>(&path, P0, Request::Act(Bump)).unwrap(),
            Response::Ack
        ));

        // Loading fresh from disk sees version 1 and total 1.
        let response = FileSession::handle::<CountToThree>(&path, P1, Request::Query).unwrap();
        assert_eq!(state_of(&response), (1, 1));
    }

    #[test]
    fn a_rejected_action_does_not_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.json");
        let path = FileSession::create(CountToThree, Some(path), 0).unwrap();

        // Seat 1 is not active first (total 0 -> seat 0), so this is rejected
        // and nothing is written.
        assert!(matches!(
            FileSession::handle::<CountToThree>(&path, P1, Request::Act(Bump)).unwrap(),
            Response::Error(_)
        ));
        let response = FileSession::handle::<CountToThree>(&path, P0, Request::Query).unwrap();
        assert_eq!(state_of(&response), (0, 0), "version stayed at 0");
    }

    #[test]
    fn handle_errors_on_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let result = FileSession::handle::<CountToThree>(&path, P0, Request::Query);
        assert!(matches!(result, Err(Error::NotFound(_))));
    }

    #[test]
    fn a_full_game_reaches_a_terminal_state_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.json");
        let path = FileSession::create(CountToThree, Some(path), 0).unwrap();

        for round in 0..3 {
            let seat = PlayerId::new(round % 2);
            FileSession::handle::<CountToThree>(&path, seat, Request::Act(Bump)).unwrap();
        }
        let response = FileSession::handle::<CountToThree>(&path, P0, Request::Query).unwrap();
        assert_eq!(
            state_of(&response),
            (3, 3),
            "three bumps, version 3, total 3"
        );
    }
}
