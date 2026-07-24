//! [`SessionApp`]: the interactive dashboard over a [`Simulator`].
//!
//! A superset of [`SimulationRunner`](crate::SimulationRunner): the same
//! viewport/stats/log dashboard, plus a setup modal (pick Human or an AI type
//! per seat), Auto/Step run modes with an adjustable speed, a reset, and a
//! human seat that plays through the action menu. One backend-generic
//! [`App`], so the same session drives a real terminal
//! (`retroglyph-crossterm`) and the in-browser demos (`retroglyph-terminal-wasm`)
//! unchanged.

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_widgets::{List, ListState, Modal, StatefulWidget, Theme};
use turnbase::{Determinize, Game, PlayerId};
use turnbase_bots::{Bot, Ismcts, Mcts, RandomBot};
use turnbase_match::{PlayerAgent, Simulator};

use crate::PrintableGame;
use crate::dashboard::{Layout, draw_board_stats_log, print_rows};

/// Search budget for the [`Mcts`]/[`Ismcts`] bot options offered in the setup
/// modal. Small, so a move computes within a frame and the demo stays
/// responsive rather than hitching on a deep search.
const SEARCH_ITERATIONS: u32 = 100;

/// Auto-mode tick intervals, slowest first. The setup/status speed control
/// indexes this; a higher index is faster (a shorter interval).
const SPEEDS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_millis(600),
    Duration::from_millis(380),
    Duration::from_millis(220),
    Duration::from_millis(110),
];

/// The default speed index into [`SPEEDS`].
const DEFAULT_SPEED: usize = 2;

/// One selectable AI type for a seat.
///
/// Names an AI and how to build it for a seat seed. Carried by [`SessionApp`]
/// so the setup modal can offer a per-game set of bots (see
/// [`random_bot`]/[`mcts_bot`]/[`ismcts_bot`]).
pub struct BotOption<G: Game> {
    name: &'static str,
    make: Box<dyn Fn(u64) -> Box<dyn Bot<G>>>,
}

impl<G: Game> BotOption<G> {
    /// Names an AI type and how to build a fresh instance seeded from a seat
    /// seed.
    pub fn new(name: &'static str, make: impl Fn(u64) -> Box<dyn Bot<G>> + 'static) -> Self {
        Self {
            name,
            make: Box::new(make),
        }
    }

    /// The label shown in the setup modal.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

/// The uniform-random bot option, available for every game.
#[must_use]
pub fn random_bot<G: Game>() -> BotOption<G> {
    BotOption::new("Random", |seed| Box::new(RandomBot::new(seed)))
}

/// The MCTS bot option, for any game whose state and actions are [`Clone`].
#[must_use]
pub fn mcts_bot<G>() -> BotOption<G>
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    BotOption::new("MCTS", |seed| Box::new(Mcts::new(SEARCH_ITERATIONS, seed)))
}

/// The information-set MCTS bot option, for a game that implements
/// [`Determinize`] (so it can search under hidden information).
#[must_use]
pub fn ismcts_bot<G>() -> BotOption<G>
where
    G: Determinize,
    G::State: Clone,
    G::Action: Clone,
{
    BotOption::new("ISMCTS", |seed| {
        Box::new(Ismcts::new(SEARCH_ITERATIONS, seed))
    })
}

/// The bot set every `Clone`-stated game can offer: [`random_bot`] and
/// [`mcts_bot`]. A game that also implements [`Determinize`] can push
/// [`ismcts_bot`] on top.
#[must_use]
pub fn standard_bots<G>() -> Vec<BotOption<G>>
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    vec![random_bot(), mcts_bot()]
}

/// Who controls a seat, as chosen in the setup modal: a human, or the AI type
/// at this index into [`SessionApp`]'s bot options.
#[derive(Clone, Copy)]
enum SeatKind {
    Human,
    Ai(usize),
}

const fn kind_index(kind: SeatKind) -> usize {
    match kind {
        SeatKind::Human => 0,
        SeatKind::Ai(index) => index + 1,
    }
}

const fn index_kind(index: usize) -> SeatKind {
    match index {
        0 => SeatKind::Human,
        other => SeatKind::Ai(other - 1),
    }
}

/// Mixes a per-seat bot seed off the match seed, so two bot seats do not share
/// a random stream.
fn seat_seed(seed: u64, seat: u32) -> u64 {
    seed ^ u64::from(seat)
        .wrapping_add(1)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// Builds a [`Simulator`] from a seat configuration, cloning the rules so the
/// caller keeps its own copy for the next rebuild.
fn build_sim<G>(game: &G, seats: &[SeatKind], bots: &[BotOption<G>], seed: u64) -> Simulator<G>
where
    G: Game + Clone,
{
    let mut agents = HashMap::new();
    for (seat, kind) in seats.iter().enumerate() {
        let index = u32::try_from(seat).expect("seat index fits in u32");
        let id = PlayerId::new(index);
        let agent = match *kind {
            SeatKind::Human => PlayerAgent::Human,
            SeatKind::Ai(bot) => PlayerAgent::Ai((bots[bot].make)(seat_seed(seed, index))),
        };
        agents.insert(id, agent);
    }
    Simulator::new(game.clone(), seed, agents)
}

/// Returns the key codes of the key-*down* events in `events`.
fn down_keys(events: &[Event]) -> impl Iterator<Item = KeyCode> + '_ {
    events.iter().filter_map(|event| match event {
        Event::Key(key) if key.is_down() => Some(key.code),
        _ => None,
    })
}

/// An interactive session over a [`Simulator`]: the dashboard plus a setup
/// modal, Auto/Step control, speed, reset, and a human-playable seat.
///
/// Fixes the dashboard's viewing seat at the lowest human seat (or a neutral
/// spectator if there is none) each time the match is (re)built, so a human
/// never sees another seat's hidden information. That single fixed viewer means
/// a hidden-info game configured with two or more human seats is local
/// pass-and-play that shows one seat's view throughout; it is not safe for
/// competitive hot-seat play of a game with private state (the same limitation
/// the CLI's text `play` documents).
pub struct SessionApp<G>
where
    G: PrintableGame + Clone,
    G::Action: Debug,
{
    game: G,
    bots: Vec<BotOption<G>>,
    seats: Vec<SeatKind>,
    // The seat config as it was when the setup modal opened, restored if the
    // modal is cancelled with Escape.
    setup_backup: Vec<SeatKind>,
    seed: u64,
    auto: bool,
    paused: bool,
    speed: usize,
    ai_elapsed: Duration,
    sim: Simulator<G>,
    viewer: Option<PlayerId>,
    selected: usize,
    setup: Option<usize>,
    exit: bool,
}

impl<G> SessionApp<G>
where
    G: PrintableGame + Clone,
    G::Action: Debug,
{
    /// Builds a session for `game` with the given `bots` available as AI types
    /// (at least [`random_bot`] is always included), every seat an AI, Auto
    /// mode, seeded from `seed`.
    #[must_use]
    pub fn new(game: G, bots: Vec<BotOption<G>>, seed: u64) -> Self {
        let mut bots = bots;
        if bots.is_empty() {
            bots.push(random_bot());
        }
        let seats = vec![SeatKind::Ai(0); game.num_players()];
        let sim = build_sim(&game, &seats, &bots, seed);
        let viewer = sim.primary_human();
        Self {
            game,
            bots,
            setup_backup: seats.clone(),
            seats,
            seed,
            auto: true,
            paused: false,
            speed: DEFAULT_SPEED,
            ai_elapsed: Duration::ZERO,
            sim,
            viewer,
            selected: 0,
            setup: None,
            exit: false,
        }
    }

    /// Marks `seat` human (it plays through the action menu). Out-of-range
    /// seats are ignored.
    #[must_use]
    pub fn with_human_seat(mut self, seat: usize) -> Self {
        if seat < self.seats.len() {
            self.seats[seat] = SeatKind::Human;
            self.rebuild();
        }
        self
    }

    /// Opens (or closes) the setup modal on start, so a native player
    /// configures seats before the match runs.
    #[must_use]
    pub fn with_setup_open(mut self, open: bool) -> Self {
        if open {
            self.setup_backup = self.seats.clone();
            self.setup = Some(0);
        } else {
            self.setup = None;
        }
        self
    }

    /// Starts in Step mode (advance one decision per Space) rather than Auto.
    #[must_use]
    pub const fn with_step_mode(mut self) -> Self {
        self.auto = false;
        self
    }

    /// Whether the match has ended and the setup modal is closed (so a demo
    /// harness can decide when to restart).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.setup.is_none() && self.sim.is_terminal()
    }

    /// Unwraps to the underlying [`Simulator`] at its current state.
    #[must_use]
    pub fn into_simulator(self) -> Simulator<G> {
        self.sim
    }

    /// Rebuilds the match from the current seats and seed (keeping the seed),
    /// e.g. after the setup modal is confirmed.
    fn rebuild(&mut self) {
        self.sim = build_sim(&self.game, &self.seats, &self.bots, self.seed);
        self.viewer = self.sim.primary_human();
        self.selected = 0;
        self.ai_elapsed = Duration::ZERO;
        self.paused = false;
    }

    /// Restarts with a fresh seed, keeping the seat configuration.
    fn reset(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        self.rebuild();
    }

    /// The AI type label for a seat kind (Human, or the bot's name).
    fn kind_label(&self, kind: SeatKind) -> &'static str {
        match kind {
            SeatKind::Human => "Human",
            SeatKind::Ai(index) => self.bots[index].name(),
        }
    }

    fn handle_controls(&mut self, events: &[Event]) {
        for key in down_keys(events) {
            match key {
                KeyCode::Escape => self.exit = true,
                KeyCode::Char('c' | 'C') => {
                    self.setup_backup = self.seats.clone();
                    self.setup = Some(0);
                }
                KeyCode::Char('r' | 'R') => self.reset(),
                KeyCode::Char('m' | 'M') | KeyCode::Tab => {
                    self.auto = !self.auto;
                    self.paused = false;
                    self.ai_elapsed = Duration::ZERO;
                }
                KeyCode::Char('p' | 'P') => {
                    if self.auto {
                        self.paused = !self.paused;
                    }
                }
                KeyCode::Char('+' | '=') => self.speed = (self.speed + 1).min(SPEEDS.len() - 1),
                KeyCode::Char('-' | '_') => self.speed = self.speed.saturating_sub(1),
                KeyCode::Char(' ') => self.step_once(),
                _ => {}
            }
        }
    }

    /// Advances one decision if the active seat is a bot or chance node (never
    /// for a human seat, which acts through the menu). The only way to progress
    /// in Step mode, and a manual nudge in Auto.
    fn step_once(&mut self) {
        if !self.sim.is_terminal() && self.sim.awaiting_human().is_none() {
            let _ = self.sim.step();
            self.ai_elapsed = Duration::ZERO;
        }
    }

    fn tick_ai(&mut self, frame: &Frame) {
        self.ai_elapsed = self.ai_elapsed.saturating_add(frame.delta);
        if self.ai_elapsed >= SPEEDS[self.speed] {
            self.ai_elapsed = Duration::ZERO;
            let _ = self.sim.step();
        }
    }

    fn handle_action_menu(&mut self, player: PlayerId, events: &[Event]) {
        let count = self
            .sim
            .game()
            .legal_actions(self.sim.state(), player)
            .len();
        if count > 0 {
            self.selected = self.selected.min(count - 1);
        }

        let mut confirm = false;
        for key in down_keys(events) {
            match key {
                KeyCode::Up if count > 0 => {
                    self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
                }
                KeyCode::Down if count > 0 => self.selected = (self.selected + 1) % count,
                KeyCode::Enter => confirm = true,
                _ => {}
            }
        }

        if confirm && count > 0 {
            let mut actions = self.sim.game().legal_actions(self.sim.state(), player);
            let action = actions.swap_remove(self.selected);
            let _ = self.sim.select_human_action(player, action);
            self.selected = 0;
        }
    }

    fn handle_setup(&mut self, events: &[Event]) {
        let seats = self.seats.len();
        let kinds = self.bots.len() + 1;
        let mut cursor = self.setup.unwrap_or(0).min(seats.saturating_sub(1));
        for key in down_keys(events) {
            match key {
                KeyCode::Up if seats > 0 => {
                    cursor = cursor.checked_sub(1).unwrap_or(seats - 1);
                }
                KeyCode::Down if seats > 0 => cursor = (cursor + 1) % seats,
                KeyCode::Left => {
                    let next = (kind_index(self.seats[cursor]) + kinds - 1) % kinds;
                    self.seats[cursor] = index_kind(next);
                }
                KeyCode::Right => {
                    let next = (kind_index(self.seats[cursor]) + 1) % kinds;
                    self.seats[cursor] = index_kind(next);
                }
                KeyCode::Enter => {
                    self.setup = None;
                    self.rebuild();
                    return;
                }
                KeyCode::Escape => {
                    // Cancel: discard the edits made in the modal.
                    self.seats.clone_from(&self.setup_backup);
                    self.setup = None;
                    return;
                }
                _ => {}
            }
        }
        self.setup = Some(cursor);
    }

    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.reset_style();
        let layout = Layout::new(term.area());
        let view = self.sim.game().view(self.sim.state(), self.viewer);
        draw_board_stats_log(
            self.sim.game(),
            &view,
            self.sim.log_history(),
            term,
            &layout,
        );
        self.draw_actions(term, &layout);
        self.draw_status(term, &layout);
        if let Some(cursor) = self.setup {
            self.draw_setup(term, cursor);
        }
    }

    fn draw_actions<B: Backend>(&self, term: &mut Terminal<B>, layout: &Layout) {
        term.print(layout.actions.left(), layout.actions.top(), "-- actions --");
        if self.sim.is_terminal() {
            print_rows(
                term,
                layout.actions,
                1,
                std::iter::once("match over -- r restart, c config".to_owned()),
            );
        } else if let Some(player) = self.sim.awaiting_human() {
            let actions = self.sim.game().legal_actions(self.sim.state(), player);
            let game = self.sim.game();
            let selected = self.selected.min(actions.len().saturating_sub(1));
            let labels = actions.iter().enumerate().map(|(row, action)| {
                let marker = if row == selected { '>' } else { ' ' };
                format!("{marker} {}", game.format_action(action))
            });
            print_rows(term, layout.actions, 1, labels);
        } else {
            let label = if !self.auto {
                "(step: press Space)"
            } else if self.paused {
                "(paused: p to resume)"
            } else {
                "(AI thinking...)"
            };
            print_rows(term, layout.actions, 1, std::iter::once(label.to_owned()));
        }
    }

    fn draw_status<B: Backend>(&self, term: &mut Terminal<B>, layout: &Layout) {
        let mode = if self.auto {
            if self.paused { "Auto(paused)" } else { "Auto" }
        } else {
            "Step"
        };
        let speed = if self.auto {
            format!("  speed {}/{}", self.speed + 1, SPEEDS.len())
        } else {
            String::new()
        };
        let turn = if self.sim.is_terminal() {
            "over".to_owned()
        } else if let Some(player) = self.sim.awaiting_human() {
            format!("P{} (you)", player.index())
        } else {
            "AI".to_owned()
        };
        let text = format!(
            " {mode}{speed}  |  turn: {turn}  |  Space step  m mode  p pause  +/- speed  c config  r reset  Esc quit"
        );

        let width = usize::from(layout.status.width());
        let mut bar: String = text.chars().take(width).collect();
        while bar.chars().count() < width {
            bar.push(' ');
        }
        let theme = Theme::DARK;
        term.reset_style().fg(theme.fg).bg(theme.title_bg);
        term.print(layout.status.left(), layout.status.top(), &bar);
        term.reset_style();
    }

    fn draw_setup<B: Backend>(&self, term: &mut Terminal<B>, cursor: usize) {
        let theme = Theme::DARK;
        let seats = self.seats.len();
        #[expect(clippy::cast_possible_truncation, reason = "seat counts are tiny")]
        let rows = seats as u16;
        let width = 44;
        let height = rows.saturating_add(5);
        let inner = Modal::new(width, height)
            .theme(theme)
            .title("Session Setup")
            .render(term.area(), term);

        let items: Vec<String> = self
            .seats
            .iter()
            .enumerate()
            .map(|(seat, kind)| format!("Seat {seat}    {}", self.kind_label(*kind)))
            .collect();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let list_rect = Rect::new(
            inner.left(),
            inner.top(),
            inner.width(),
            rows.min(inner.height()),
        );
        let mut state = ListState::new();
        state.select(Some(cursor));
        List::new(&refs)
            .theme(theme)
            .render(list_rect, term, &mut state);

        let hint_rect = Rect::new(
            inner.left(),
            inner.bottom().saturating_sub(1),
            inner.width(),
            1,
        );
        term.reset_style().fg(theme.dim);
        print_rows(
            term,
            hint_rect,
            0,
            std::iter::once("arrows pick   Enter start   Esc cancel".to_owned()),
        );
        term.reset_style();
    }
}

impl<G, B> App<B> for SessionApp<G>
where
    G: PrintableGame + Clone,
    G::Action: Debug,
    B: Backend,
{
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        let events: Vec<Event> = term.drain_events().collect();

        if self.setup.is_some() {
            self.handle_setup(&events);
        } else {
            self.handle_controls(&events);
            if !self.exit && !self.sim.is_terminal() {
                match self.sim.awaiting_human() {
                    Some(player) => self.handle_action_menu(player, &events),
                    None => {
                        if self.auto && !self.paused {
                            self.tick_ai(frame);
                        }
                    }
                }
            }
        }

        self.draw(term);
        let _ = term.present();

        if self.exit {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

/// Runs `app` on a real terminal via `retroglyph-crossterm` until it quits.
///
/// # Errors
/// Returns an `std::io::Error` if the terminal backend fails to initialize.
#[cfg(feature = "crossterm")]
pub fn run_session<G>(app: SessionApp<G>) -> std::io::Result<()>
where
    G: PrintableGame + Clone,
    G::Action: Debug,
{
    retroglyph_crossterm::Crossterm::run(app)
}
