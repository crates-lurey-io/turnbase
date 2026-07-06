//! Reference games implemented against the Turnbase engine.
//!
//! These validate the engine's traits against real rules and serve as worked
//! examples. This crate is not published (`publish = false`); it exists for
//! tests, benchmarks, and demos across the workspace.

pub mod high_card;
pub mod rock_paper_scissors;
pub mod tic_tac_toe;

pub use high_card::HighCard;
pub use rock_paper_scissors::RockPaperScissors;
pub use tic_tac_toe::{Board, Cell, Move, TicTacToe};
