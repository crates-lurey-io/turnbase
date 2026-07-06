//! Engine error type.

use crate::player::PlayerId;

/// Errors returned by the engine's checked entry points.
///
/// The in-place `apply` primitive assumes a legal action and does not return
/// errors; these surface from checked helpers such as `apply_cloned`, which
/// validate before mutating. More variants may be added, so match with a
/// wildcard arm.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Error {
    /// The action is not legal for `player` in the current state.
    IllegalAction {
        /// The seat that attempted the action.
        player: PlayerId,
    },
    /// `player` is not among the active players owed a decision right now.
    NotActive {
        /// The seat that attempted to act out of turn.
        player: PlayerId,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IllegalAction { player } => {
                write!(f, "illegal action for {player}")
            }
            Self::NotActive { player } => {
                write!(f, "{player} is not active")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::player::PlayerId;

    #[test]
    fn display_mentions_the_player() {
        let e = Error::IllegalAction {
            player: PlayerId::new(1),
        };
        assert_eq!(e.to_string(), "illegal action for p1");
    }
}
