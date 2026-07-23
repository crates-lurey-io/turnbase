//! A fixed-layout terminal dashboard for [`Simulator`], built directly on
//! `retroglyph-core`'s `App`/`Terminal` contract.
//!
//! There is no intermediate widget or layout-engine layer here: the screen is
//! split into four rects by plain arithmetic (see [`Layout`]), and each panel
//! is drawn with plain `Terminal::print` calls. A game opts in by
//! implementing [`PrintableGame`] on its [`turnbase::Game`] and drawing its
//! board into the rect [`SimulationRunner`] hands it.
//!
//! No panel clears its rect before drawing: `Terminal::present` already wipes
//! the whole buffer after every frame (retroglyph's immediate-mode
//! contract), so the surface each `update` call draws onto is already blank.

use std::fmt::Debug;
use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use turnbase::{Game, PlayerId};
use turnbase_match::Simulator;

/// A [`Game`] that knows how to render itself, for [`SimulationRunner`].
///
/// Every method takes `&self` (the rules) and an explicit `view`: what a
/// player or spectator is allowed to see ([`Game::view`]), never the raw
/// `Self::State`. Games with no hidden information can have `View` be a
/// clone of `State` (as the perfect-information examples in this workspace
/// already do); games with hidden hands or decks get that redaction for
/// free, since [`SimulationRunner`] always renders from one fixed seat's
/// perspective (see [`Simulator::primary_human`]) rather than the full
/// state.
pub trait PrintableGame: Game {
    /// Draws the board (map, cards, whatever the game is) into `area`.
    ///
    /// Implementations should stay within `area`; the rest of the screen is
    /// reserved for the dashboard.
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect);

    /// Key/value pairs summarizing `view` (scores, resources, hand sizes) for
    /// the stats panel, in display order.
    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)>;

    /// Renders `action` as one line of menu text for the action-select panel.
    fn format_action(&self, action: &Self::Action) -> String;
}

/// The four fixed panels of the [`SimulationRunner`] dashboard.
///
/// Plain rect arithmetic, not a constraint solver: viewport left 70%, stats
/// and action menu stacked in the remaining top-right 30%, log strip across
/// the bottom 25%, each pair separated by a one-cell [`GUTTER`] so adjacent
/// panels' text never runs together with no visible boundary. A game with
/// different needs is expected to implement [`PrintableGame`] directly
/// against `retroglyph-core` rather than configuring this layout, per the
/// crate's no-layout-engine stance.
struct Layout {
    viewport: Rect,
    stats: Rect,
    actions: Rect,
    log: Rect,
}

/// Blank cells left between adjacent panels.
const GUTTER: u16 = 1;

impl Layout {
    const fn new(full: Rect) -> Self {
        let width = full.width();
        let height = full.height();
        let left_width = width * 7 / 10;
        let right_x = left_width.saturating_add(GUTTER);
        let right_width = width.saturating_sub(right_x);
        let log_height = height / 4;
        let top_height = height.saturating_sub(log_height).saturating_sub(GUTTER);
        let log_y = top_height.saturating_add(GUTTER);
        let stats_height = top_height / 2;
        let actions_y = stats_height.saturating_add(GUTTER);
        let actions_height = top_height.saturating_sub(actions_y);

        Self {
            viewport: Rect::new(0, 0, left_width, top_height),
            stats: Rect::new(right_x, 0, right_width, stats_height),
            actions: Rect::new(right_x, actions_y, right_width, actions_height),
            log: Rect::new(0, log_y, width, log_height),
        }
    }
}

/// Prints `lines` one per row starting `top_offset` rows below `rect`'s top
/// edge, at `rect`'s left edge. Rows that would fall at or past `rect`'s
/// bottom edge are dropped rather than drawn, and each line is truncated to
/// `rect`'s width, so overlong content clips instead of spilling into the
/// next panel or the row below.
///
/// `Terminal::print` only wraps at the *grid's* width, not an arbitrary
/// rect's; without this truncation, a line longer than `rect` but shorter
/// than the full terminal would silently wrap into the row directly below
/// at the same x, corrupting whatever the next `line` writes there instead
/// of visibly clipping.
fn print_rows<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    top_offset: u16,
    lines: impl IntoIterator<Item = String>,
) {
    let max_chars = usize::from(rect.width());
    let mut y = rect.top().saturating_add(top_offset);
    for line in lines {
        if y >= rect.bottom() {
            break;
        }
        let clipped: String = line.chars().take(max_chars).collect();
        term.print(rect.left(), y, &clipped);
        y = y.saturating_add(1);
    }
}

/// Drives a [`Simulator`] behind a fixed dashboard.
///
/// The game's own viewport sits on the left, a stats panel and (when a human
/// is up) an action-select menu stack on the right, and a scrolling log strip
/// runs along the bottom.
///
/// Implements [`App`] for every [`Backend`], so the same runner drives a real
/// terminal (`retroglyph-crossterm`) or an in-memory [`retroglyph_core::backend::Headless`]
/// test session unchanged; [`run`] is the convenience entry point for the
/// former. Escape ends the loop early regardless of whose turn it is; once
/// the match reaches a terminal state the dashboard keeps rendering the
/// final position (rather than exiting the instant it happens, which would
/// hide it) and waits for Enter or Escape to close.
pub struct SimulationRunner<G: PrintableGame> {
    simulator: Simulator<G>,
    selected: usize,
    ai_tick: Duration,
    ai_elapsed: Duration,
    viewer: Option<PlayerId>,
}

impl<G: PrintableGame> SimulationRunner<G> {
    /// Wraps `simulator`, polling an AI-controlled active seat for a move
    /// once every `ai_tick` of wall-clock time (via [`Frame::delta`]) rather
    /// than on every single frame, so AI turns are visible instead of
    /// flashing past.
    ///
    /// Fixes the dashboard's viewing seat for the whole match at
    /// [`Simulator::primary_human`] (or a neutral spectator if there is no
    /// human seat), computed once here rather than every frame.
    #[must_use]
    pub fn new(simulator: Simulator<G>, ai_tick: Duration) -> Self {
        let viewer = simulator.primary_human();
        Self {
            simulator,
            selected: 0,
            ai_tick,
            ai_elapsed: Duration::ZERO,
            viewer,
        }
    }

    /// Returns whether the wrapped match has reached a terminal state.
    ///
    /// True while the dashboard is showing its "match over" prompt and
    /// waiting for Enter/Escape, per [`SimulationRunner`]'s own docs.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.simulator.is_terminal()
    }

    /// Unwraps the runner, returning the [`Simulator`] at its current state
    /// (e.g. after the loop exits, to inspect the final position or log).
    #[must_use]
    pub fn into_simulator(self) -> Simulator<G> {
        self.simulator
    }
}

impl<G, B> App<B> for SimulationRunner<G>
where
    G: PrintableGame,
    G::Action: Debug,
    B: Backend,
{
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        let events: Vec<Event> = term.drain_events().collect();

        if key_pressed(&events, KeyCode::Escape) {
            return Flow::Exit;
        }

        let over = self.simulator.is_terminal();
        if over {
            if key_pressed(&events, KeyCode::Enter) {
                return Flow::Exit;
            }
        } else {
            match self.simulator.awaiting_human() {
                Some(player) => self.handle_human_input(player, &events),
                None => self.tick_ai(frame),
            }
        }

        term.reset_style();
        let layout = Layout::new(term.area());
        let view = self
            .simulator
            .game()
            .view(self.simulator.state(), self.viewer);

        self.simulator
            .game()
            .draw_viewport(&view, term, layout.viewport);

        term.print(layout.stats.left(), layout.stats.top(), "-- stats --");
        let stats = self.simulator.game().get_stats(&view);
        print_rows(
            term,
            layout.stats,
            1,
            stats
                .into_iter()
                .map(|(key, value)| format!("{key}: {value}")),
        );

        term.print(layout.actions.left(), layout.actions.top(), "-- actions --");
        if over {
            print_rows(
                term,
                layout.actions,
                1,
                std::iter::once("match over -- Enter/Esc to exit".to_owned()),
            );
        } else if let Some(player) = self.simulator.awaiting_human() {
            let actions = self
                .simulator
                .game()
                .legal_actions(self.simulator.state(), player);
            let game = self.simulator.game();
            let selected = self.selected;
            let labels = actions.iter().enumerate().map(|(row, action)| {
                let marker = if row == selected { '>' } else { ' ' };
                format!("{marker} {}", game.format_action(action))
            });
            print_rows(term, layout.actions, 1, labels);
        } else {
            print_rows(
                term,
                layout.actions,
                1,
                std::iter::once("(AI thinking...)".to_owned()),
            );
        }

        let capacity = usize::from(layout.log.height());
        let history = self.simulator.log_history();
        let start = history.len().saturating_sub(capacity);
        print_rows(term, layout.log, 0, history[start..].iter().cloned());

        let _ = term.present();

        Flow::Continue
    }
}

/// Returns `true` if `events` contains a press or auto-repeat of `code`.
fn key_pressed(events: &[Event], code: KeyCode) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::Key(key) if key.is_down() && key.code == code))
}

impl<G> SimulationRunner<G>
where
    G: PrintableGame,
    G::Action: Debug,
{
    /// Reads arrow keys to move the selection and Enter to commit it,
    /// re-reading `legal_actions` fresh each time rather than caching the
    /// list across frames, since a game's legal set can change out from
    /// under a stale index (e.g. another simultaneous seat just resolved).
    fn handle_human_input(&mut self, player: turnbase::PlayerId, events: &[Event]) {
        let count = self
            .simulator
            .game()
            .legal_actions(self.simulator.state(), player)
            .len();
        if count > 0 {
            self.selected = self.selected.min(count - 1);
        }

        let mut confirm = false;
        for event in events {
            let Event::Key(key) = event else { continue };
            if !key.is_down() {
                continue;
            }
            match key.code {
                KeyCode::Up if count > 0 => {
                    self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
                }
                KeyCode::Down if count > 0 => {
                    self.selected = (self.selected + 1) % count;
                }
                KeyCode::Enter => confirm = true,
                _ => {}
            }
        }

        if confirm && count > 0 {
            let mut actions = self
                .simulator
                .game()
                .legal_actions(self.simulator.state(), player);
            let action = actions.swap_remove(self.selected);
            let _ = self.simulator.select_human_action(player, action);
            self.selected = 0;
        }
    }

    /// Advances the AI clock by one frame, stepping the simulator once
    /// `ai_tick` has accumulated.
    fn tick_ai(&mut self, frame: &Frame) {
        self.ai_elapsed = self.ai_elapsed.saturating_add(frame.delta);
        if self.ai_elapsed >= self.ai_tick {
            self.ai_elapsed = Duration::ZERO;
            let _ = self.simulator.step();
        }
    }
}

/// Runs `simulator` on a real terminal via `retroglyph-crossterm`, polling
/// an AI-controlled seat every `ai_tick`, until the match ends or the
/// terminal closes.
///
/// # Errors
/// Returns an `std::io::Error` if the terminal backend fails to initialize.
pub fn run<G>(simulator: Simulator<G>, ai_tick: Duration) -> std::io::Result<()>
where
    G: PrintableGame,
    G::Action: Debug,
{
    retroglyph_crossterm::Crossterm::run(SimulationRunner::new(simulator, ai_tick))
}
