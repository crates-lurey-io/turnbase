//! Tic-tac-toe: the simplest game that exercises the whole [`Game`] trait.
//!
//! Singleton `active_players`, a nine-cell action space, no hidden information,
//! no randomness, no triggers. Seat 0 plays `X` and moves first; seat 1 plays
//! `O`.

use std::fmt;

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, Determinize, Game, PlayerId, Prng, Reversible};

/// One square of the board.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Cell {
    /// Unclaimed square.
    #[default]
    Empty,
    /// Claimed by seat 0.
    X,
    /// Claimed by seat 1.
    O,
}

/// Placing the moving player's mark on cell `0..9` (row-major).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Move(pub u8);

/// A board position: the nine cells plus how many marks have been placed.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Board {
    cells: [Cell; 9],
    marks: u8,
}

/// The rules of tic-tac-toe. Carries no configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct TicTacToe;

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

impl Board {
    /// Returns the contents of cell `index`.
    ///
    /// # Panics
    /// Panics if `index >= 9`.
    #[must_use]
    pub const fn cell(&self, index: usize) -> Cell {
        self.cells[index]
    }

    /// Returns all nine cells in row-major order.
    #[must_use]
    pub const fn cells(&self) -> &[Cell; 9] {
        &self.cells
    }

    /// Returns how many marks have been placed so far.
    #[must_use]
    pub const fn marks_placed(&self) -> u8 {
        self.marks
    }

    /// Returns the mark occupying a completed line, if any.
    #[must_use]
    fn winner(&self) -> Option<Cell> {
        for [a, b, c] in LINES {
            let mark = self.cells[a];
            if mark != Cell::Empty && self.cells[b] == mark && self.cells[c] == mark {
                return Some(mark);
            }
        }
        None
    }

    const fn is_full(&self) -> bool {
        self.marks as usize == self.cells.len()
    }
}

impl TicTacToe {
    /// The mark the given seat plays.
    const fn mark_of(player: PlayerId) -> Cell {
        if player.index() == 0 {
            Cell::X
        } else {
            Cell::O
        }
    }
}

impl Game for TicTacToe {
    type State = Board;
    type Action = Move;
    type View = Board;

    fn new_initial_state(&self, _seed: u64) -> Self::State {
        Board {
            cells: [Cell::Empty; 9],
            marks: 0,
        }
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(u32::from(state.marks % 2)))
        }
    }

    fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) {
            return Vec::new();
        }
        (0..9u8)
            .filter(|&i| state.cells[i as usize] == Cell::Empty)
            .map(Move)
            .collect()
    }

    fn is_legal(&self, state: &Self::State, _player: PlayerId, action: &Self::Action) -> bool {
        let index = action.0 as usize;
        !self.is_terminal(state) && index < 9 && state.cells[index] == Cell::Empty
    }

    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        state.cells[action.0 as usize] = Self::mark_of(player);
        state.marks += 1;
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.winner().is_some() || state.is_full()
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        match state.winner() {
            Some(mark) if mark == Self::mark_of(player) => 1.0,
            Some(_) => -1.0,
            None => 0.0,
        }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        state.clone()
    }
}

impl Reversible for TicTacToe {
    /// The cell that was filled; reversing clears it and decrements the count.
    /// There is no RNG to rewind (tic-tac-toe is deterministic).
    type UndoRecord = Move;

    fn apply_undoable(
        &self,
        state: &mut Self::State,
        player: PlayerId,
        action: Self::Action,
    ) -> Self::UndoRecord {
        self.apply(state, player, action);
        action
    }

    fn undo(&self, state: &mut Self::State, record: Self::UndoRecord) {
        state.cells[record.0 as usize] = Cell::Empty;
        state.marks -= 1;
    }
}

impl Determinize for TicTacToe {
    /// Nothing is hidden, so a determinization is just the true state. Lets a
    /// hidden-info bot (`Ismcts`) run on tic-tac-toe as a baseline.
    fn determinize(
        &self,
        state: &Self::State,
        _observer: PlayerId,
        _rng: &mut Prng,
    ) -> Self::State {
        state.clone()
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => ".",
            Self::X => "X",
            Self::O => "O",
        })
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in 0..3 {
            for col in 0..3 {
                write!(f, "{}", self.cells[row * 3 + col])?;
            }
            if row < 2 {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
// reward() returns exactly 0.0 or ±1.0 (literal, representable), so equality
// comparisons against those constants are exact, not approximate.
#[allow(clippy::float_cmp)]
mod tests {
    use super::{Board, Cell, Move, TicTacToe};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Plays the listed cells in order, alternating seats starting with P0.
    fn play(moves: &[u8]) -> Board {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        for (i, &cell) in moves.iter().enumerate() {
            let player = if i % 2 == 0 { P0 } else { P1 };
            assert!(game.is_legal(&state, player, &Move(cell)));
            game.apply(&mut state, player, Move(cell));
        }
        state
    }

    #[test]
    fn seats_alternate_and_stop_at_terminal() {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        assert!(game.active_players(&state).contains(P0));
        game.apply(&mut state, P0, Move(4));
        assert!(game.active_players(&state).contains(P1));
    }

    #[test]
    fn legal_actions_shrink_as_cells_fill() {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        assert_eq!(game.legal_actions(&state, P0).len(), 9);
        game.apply(&mut state, P0, Move(0));
        assert_eq!(game.legal_actions(&state, P1).len(), 8);
    }

    #[test]
    fn occupied_and_out_of_range_moves_are_illegal() {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        game.apply(&mut state, P0, Move(0));
        assert!(!game.is_legal(&state, P1, &Move(0)));
        assert!(!game.is_legal(&state, P1, &Move(9)));
        assert!(game.apply_cloned(&state, P1, Move(0)).is_err());
    }

    #[test]
    fn row_win_is_terminal_and_rewarded() {
        // P0: 0,1,2 (top row); P1: 3,4.
        let game = TicTacToe;
        let state = play(&[0, 3, 1, 4, 2]);
        assert!(game.is_terminal(&state));
        assert_eq!(state.winner(), Some(Cell::X));
        assert_eq!(game.reward(&state, P0), 1.0);
        assert_eq!(game.reward(&state, P1), -1.0);
    }

    #[test]
    fn full_board_without_a_line_is_a_draw() {
        // X O X / X O O / O X X  -> no three in a line.
        let game = TicTacToe;
        let state = play(&[0, 1, 2, 4, 3, 5, 7, 6, 8]);
        assert!(game.is_terminal(&state));
        assert_eq!(state.winner(), None);
        assert_eq!(game.reward(&state, P0), 0.0);
        assert_eq!(game.reward(&state, P1), 0.0);
        assert!(game.legal_actions(&state, P0).is_empty());
    }

    #[test]
    fn apply_cloned_leaves_the_original_untouched() {
        let game = TicTacToe;
        let state = game.new_initial_state(0);
        let next = game.apply_cloned(&state, P0, Move(4)).unwrap();
        assert_eq!(state.cell(4), Cell::Empty);
        assert_eq!(next.cell(4), Cell::X);
    }
}
