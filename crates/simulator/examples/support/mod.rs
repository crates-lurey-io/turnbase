//! Games shared by this crate's `interactive_*`/`headless_ui` examples.
//!
//! Kept out of the library itself (and out of `examples/*.rs` proper, since
//! `impl PrintableGame for MinionBattle` would violate the orphan rule from
//! an example binary — neither the trait nor `examples::MinionBattle` are
//! local to it) and included via `#[path]` instead.
//!
//! `MinionBattle` and `Coup` reuse the workspace's own tested implementations
//! from the `examples` crate rather than reimplementing rules here, through a
//! thin newtype (`DemoMinionBattle`, `DemoCoup`) that delegates every
//! [`Game`] method to the wrapped type. That newtype exists only to satisfy
//! the orphan rule locally — putting `PrintableGame` impls directly on
//! `examples`'s types would mean giving `examples` (a UI-free reference-games
//! crate depended on by `turnbase-bots`) an optional dependency on this
//! crate's `retroglyph` stack, which is exactly the "UI-free core" boundary
//! `ARCHITECTURE.md` draws for the engine.
#![allow(dead_code)] // Each including example only uses part of this surface.

use retroglyph_core::grid::Rect;
use retroglyph_core::{Backend, Terminal};
use turnbase::{ActivePlayers, Game, PlayerId};
use turnbase_simulator::PrintableGame;

// ---------------------------------------------------------------------------
// CountToTen: the simplest demo game, defined here rather than reused (there
// is nothing to reuse it from).
// ---------------------------------------------------------------------------

/// Two seats alternately add 1..=3 to a shared total; whoever reaches 10 wins.
#[derive(Clone, Copy, Default)]
pub struct CountToTen;

/// Add `0` (amount 1) through `2` (amount 3) to the running total.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Add(pub u32);

/// The running total plus how many additions have been made.
///
/// `moves` tracks whose turn it is; `total`'s parity cannot be used for that
/// (as an earlier version of this demo did), since an amount of 2 leaves
/// parity unchanged and would let the same seat move twice in a row.
///
/// Also doubles as [`Game::View`]: `CountToTen` has no hidden information, so
/// there is nothing a view needs to redact.
#[derive(Clone, Copy, Default)]
pub struct Position {
    total: u32,
    moves: u32,
}

impl Game for CountToTen {
    type State = Position;
    type Action = Add;
    type View = Position;

    fn new_initial_state(&self, _seed: u64) -> Self::State {
        Position::default()
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(state.moves % 2))
        }
    }

    fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) {
            Vec::new()
        } else {
            vec![Add(0), Add(1), Add(2)]
        }
    }

    fn apply(&self, state: &mut Self::State, _player: PlayerId, action: Self::Action) {
        state.total += action.0 + 1;
        state.moves += 1;
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.total >= 10
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        // The seat that just moved (the previous mover) is the one who
        // crossed 10.
        let winner = (state.moves + 1) % 2;
        if player.index() == winner { 1.0 } else { -1.0 }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        *state
    }
}

impl PrintableGame for CountToTen {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        term.print(area.left(), area.top(), &format!("total: {}", view.total));
        term.print(area.left(), area.top() + 2, "first to 10 (or past it) wins");
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("total".to_owned(), view.total.to_string()),
            ("seat to move".to_owned(), (view.moves % 2).to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        format!("add {}", action.0 + 1)
    }
}

// ---------------------------------------------------------------------------
// Shared delegation helper: forwards every `Game` method on a `(Inner)`
// newtype straight to `Inner`'s own impl, so `MinionBattle`/`Coup` (foreign
// types, from the `examples` crate) can carry a local `PrintableGame` impl
// without reimplementing their rules.
// ---------------------------------------------------------------------------

macro_rules! delegate_game {
    ($wrapper:ty, $inner:ty) => {
        impl Game for $wrapper {
            type State = <$inner as Game>::State;
            type Action = <$inner as Game>::Action;
            type View = <$inner as Game>::View;

            fn new_initial_state(&self, seed: u64) -> Self::State {
                self.0.new_initial_state(seed)
            }
            fn num_players(&self) -> usize {
                self.0.num_players()
            }
            fn active_players(&self, state: &Self::State) -> ActivePlayers {
                self.0.active_players(state)
            }
            fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
                self.0.legal_actions(state, player)
            }
            fn is_legal(
                &self,
                state: &Self::State,
                player: PlayerId,
                action: &Self::Action,
            ) -> bool {
                self.0.is_legal(state, player, action)
            }
            fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
                self.0.apply(state, player, action);
            }
            fn is_terminal(&self, state: &Self::State) -> bool {
                self.0.is_terminal(state)
            }
            fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
                self.0.reward(state, player)
            }
            fn view(&self, state: &Self::State, viewer: Option<PlayerId>) -> Self::View {
                self.0.view(state, viewer)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// MinionBattle: reused from `examples::minion_battle`. Perfect information
// (View is a clone of the whole board), so nothing needs redacting; the
// interesting part is watching `EffectSystem` deathrattle cascades play out
// in the log panel.
// ---------------------------------------------------------------------------

/// Wraps [`examples::MinionBattle`] with a local [`PrintableGame`] impl.
#[derive(Clone, Copy, Default)]
pub struct DemoMinionBattle(pub examples::MinionBattle);

delegate_game!(DemoMinionBattle, examples::MinionBattle);

fn minion_line(board: &[examples::minion_battle::Minion]) -> String {
    if board.is_empty() {
        return "(empty)".to_owned();
    }
    board
        .iter()
        .map(|m| format!("[{}: {}/{}]", m.id, m.attack, m.health))
        .collect::<Vec<_>>()
        .join(" ")
}

impl PrintableGame for DemoMinionBattle {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        term.print(
            area.left(),
            area.top(),
            &format!("hero 0: {} hp    hero 1: {} hp", view.hero(0), view.hero(1)),
        );
        term.print(area.left(), area.top() + 2, "seat 0 board:");
        term.print(area.left(), area.top() + 3, &minion_line(view.board(0)));
        term.print(area.left(), area.top() + 5, "seat 1 board:");
        term.print(area.left(), area.top() + 6, &minion_line(view.board(1)));
        term.print(
            area.left(),
            area.top() + 8,
            &format!("turn {} (seat {} to move)", view.turn(), view.turn() % 2),
        );
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("hero 0".to_owned(), format!("{} hp", view.hero(0))),
            ("hero 1".to_owned(), format!("{} hp", view.hero(1))),
            ("turn".to_owned(), view.turn().to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        use examples::minion_battle::{Action as MbAction, Target};
        match action {
            MbAction::Attack { attacker, target } => {
                let target = match target {
                    Target::Hero => "hero".to_owned(),
                    Target::Minion(id) => format!("minion {id}"),
                };
                format!("attack with minion {attacker} -> {target}")
            }
            MbAction::EndTurn => "end turn".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Coup: reused from `examples::coup`. Hidden information (each seat's
// influence cards), so this is the demo that actually exercises the switch
// from `&Self::State` to `&Self::View`: `CoupView::own_hand` is only
// populated for the seat `SimulationRunner` renders from (see
// `Simulator::primary_human`), so the AI's hand never reaches the screen.
// ---------------------------------------------------------------------------

/// Wraps [`examples::Coup`] with a local [`PrintableGame`] impl.
#[derive(Clone, Copy)]
pub struct DemoCoup(pub examples::Coup);

impl Default for DemoCoup {
    fn default() -> Self {
        Self(examples::Coup::new(2))
    }
}

delegate_game!(DemoCoup, examples::Coup);

impl PrintableGame for DemoCoup {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        term.print(
            area.left(),
            area.top(),
            &format!("seat to move: p{}", view.current),
        );

        let mut y = area.top().saturating_add(2);
        for (seat, &coins) in view.coins.iter().enumerate() {
            let lost = describe_cards(&view.lost[seat]);
            term.print(
                area.left(),
                y,
                &format!(
                    "p{seat}: {coins} coins, {} influence (lost: {lost})",
                    view.influence[seat]
                ),
            );
            y = y.saturating_add(1);
        }

        y = y.saturating_add(1);
        term.print(
            area.left(),
            y,
            &format!("deck: {} cards left", view.deck_size),
        );
        y = y.saturating_add(1);
        term.print(
            area.left(),
            y,
            &format!("your hand: {}", describe_cards(&view.own_hand)),
        );
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        // Hand contents go in the (wider) viewport, not here: this panel is
        // narrow enough that a multi-card hand like "Contessa, Duke" wraps
        // into the row below and corrupts whatever's printed there.
        let mut stats: Vec<(String, String)> = view
            .coins
            .iter()
            .enumerate()
            .map(|(seat, &coins)| (format!("p{seat} coins"), coins.to_string()))
            .collect();
        stats.push(("deck".to_owned(), view.deck_size.to_string()));
        stats
    }

    fn format_action(&self, action: &Self::Action) -> String {
        use examples::coup::Action as CoupAction;
        match action {
            CoupAction::Income => "income (+1 coin)".to_owned(),
            CoupAction::ForeignAid => "foreign aid (+2 coins)".to_owned(),
            CoupAction::Coup(target) => format!("coup p{target}"),
            CoupAction::Tax => "tax, claim Duke (+3 coins)".to_owned(),
            CoupAction::Assassinate(target) => {
                format!("assassinate p{target}, claim Assassin")
            }
            CoupAction::Steal(target) => format!("steal from p{target}, claim Captain"),
            CoupAction::Exchange => "exchange, claim Ambassador".to_owned(),
            CoupAction::Return(index) => format!("return card {index}"),
            CoupAction::Pass => "pass".to_owned(),
            CoupAction::Challenge => "challenge".to_owned(),
            CoupAction::Block(character) => format!("block, claim {character:?}"),
            CoupAction::Lose(index) => format!("lose influence: card {index}"),
        }
    }
}

/// Renders a hand/discard as `"Duke, Captain"`, or `"none"` when empty (a
/// redacted opponent hand, or a seat with no influence left).
fn describe_cards(cards: &[examples::coup::Character]) -> String {
    if cards.is_empty() {
        return "none".to_owned();
    }
    cards
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
