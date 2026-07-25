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

use retroglyph_core::event::{Event, KeyCode, MouseButton, MouseEventKind};
use retroglyph_core::grid::{Pos, Rect};
use retroglyph_core::{App, Backend, Flow, Frame, Terminal};
use retroglyph_widgets::{List, ListState, Modal, StatefulWidget, Theme, offset_for_pos};
use turnbase::{Determinize, Game, PlayerId};
use turnbase_bots::{Bot, Ismcts, Mcts, RandomBot};
use turnbase_match::{PlayerAgent, Simulator};

use crate::PrintableGame;
use crate::dashboard::{
    Layout, actions_panel, draw_board_stats_log, draw_menu, log_geometry, menu_start, panel_inner,
    print_rows,
};

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

/// Log lines one wheel notch (or one touch-drag step, in the browser demos)
/// scrolls.
const WHEEL_LINES: usize = 3;

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

/// Counts the human-controlled seats. Two or more means local pass-and-play,
/// which gates each seat's reveal behind a device handoff.
fn count_humans(seats: &[SeatKind]) -> usize {
    seats
        .iter()
        .filter(|kind| matches!(kind, SeatKind::Human))
        .count()
}

/// Chooses the dashboard's viewing seat for a freshly built match: with two or
/// more human seats the viewer follows the acting seat (starting as a neutral
/// spectator, gated by a handoff), otherwise it is fixed at the single human
/// (or a spectator when every seat is AI).
fn viewer_for<G: Game>(seats: &[SeatKind], sim: &Simulator<G>) -> Option<PlayerId> {
    if count_humans(seats) >= 2 {
        None
    } else {
        sim.primary_human()
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

/// Where the log strip is scrolled to, and what the last frame drew.
///
/// One struct rather than four loose fields on [`SessionApp`]: they are only
/// ever read and written together (a scroll, an anchor, a drag), and the
/// panel's own draw is the only thing that can fill `rows` in.
#[derive(Clone, Copy, Debug)]
struct LogScroll {
    /// Lines back from the newest; 0 pins to the tail.
    offset: usize,
    /// Visible log rows as of the last draw, to size a page jump and clamp
    /// [`offset`](Self::offset).
    rows: usize,
    /// History length last seen, to keep a scrolled-back view anchored to the
    /// same lines as auto-play appends new ones.
    len_seen: usize,
    /// Whether the left button is held on the scrollbar, so pointer moves keep
    /// dragging the thumb even once the pointer slides off the strip.
    dragging: bool,
}

impl LogScroll {
    /// Pinned to the newest line, with a one-row panel assumed until the first
    /// draw reports the real height.
    const fn new() -> Self {
        Self {
            offset: 0,
            rows: 1,
            len_seen: 0,
            dragging: false,
        }
    }
}

/// What is currently in front of the dashboard. Mutually exclusive by
/// construction, so only one modal-ish state can be open at a time.
#[derive(Clone, Copy)]
enum Overlay {
    /// Nothing: the live match dashboard is in front.
    None,
    /// The seat-setup modal, carrying the cursor's seat row.
    Setup(usize),
    /// The controls help card.
    Help,
    /// A pending device handoff to the given seat, awaiting Enter to reveal.
    Handoff(PlayerId),
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
    log: LogScroll,
    // The active overlay (setup modal, help card, or a pending device
    // handoff), or None when the live dashboard is in front. One field, so the
    // three stay mutually exclusive by construction.
    overlay: Overlay,
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
        let viewer = viewer_for(&seats, &sim);
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
            log: LogScroll::new(),
            overlay: Overlay::None,
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
        self.overlay = if open {
            self.setup_backup = self.seats.clone();
            Overlay::Setup(0)
        } else {
            Overlay::None
        };
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
        matches!(self.overlay, Overlay::None) && self.sim.is_terminal()
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
        self.viewer = viewer_for(&self.seats, &self.sim);
        // A rebuild invalidates a pending handoff, but must preserve an open
        // setup modal (with_human_seat rebuilds while the modal is still up).
        if matches!(self.overlay, Overlay::Handoff(_)) {
            self.overlay = Overlay::None;
        }
        self.selected = 0;
        self.ai_elapsed = Duration::ZERO;
        self.paused = false;
        self.log.offset = 0;
        self.log.len_seen = 0;
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
                    self.overlay = Overlay::Setup(0);
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
                KeyCode::Char('?' | 'h' | 'H') => self.overlay = Overlay::Help,
                KeyCode::Char(' ') => self.step_once(),
                KeyCode::PageUp => self.scroll_log_back(self.log.rows.max(1)),
                KeyCode::PageDown => self.scroll_log_forward(self.log.rows.max(1)),
                KeyCode::Home => self.scroll_log_back(usize::MAX),
                KeyCode::End => self.log.offset = 0,
                _ => {}
            }
        }
    }

    /// Routes pointer input over the dashboard: the wheel (or a trackpad/touch
    /// scroll, which arrives as wheel events) scrolls the log, the scrollbar on
    /// its right edge is click-to-jump and drag-to-scroll, and a click in the
    /// actions panel picks a move.
    ///
    /// `layout` is the current frame's, so the rects hit-tested here are the
    /// ones the last frame drew. A drag that started on the strip keeps
    /// tracking the pointer's row even after it slides off (the usual
    /// scrollbar behavior), and only a button release ends it.
    fn handle_mouse(&mut self, layout: &Layout, events: &[Event]) {
        let total = self.sim.log_history().len();
        let geometry = log_geometry(layout.log, total);
        for event in events {
            let Event::Mouse(mouse) = event else { continue };
            let over_log = layout.log.contains_pos(mouse.position);
            match mouse.kind {
                MouseEventKind::ScrollUp if over_log => self.scroll_log_back(WHEEL_LINES),
                MouseEventKind::ScrollDown if over_log => self.scroll_log_forward(WHEEL_LINES),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(bar) = geometry.bar
                        && bar.contains_pos(mouse.position)
                    {
                        self.log.dragging = true;
                        self.drag_log_to(bar, total, geometry.visible, mouse.position);
                    } else {
                        self.click_action(layout, mouse.position);
                    }
                }
                MouseEventKind::Moved if self.log.dragging => {
                    if let Some(bar) = geometry.bar {
                        self.drag_log_to(bar, total, geometry.visible, mouse.position);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => self.log.dragging = false,
                _ => {}
            }
        }
    }

    /// Picks the action a click at `pos` in the actions panel landed on.
    ///
    /// Click to select, click the selected row again to play it, mirroring
    /// Up/Down then Enter. Two clicks rather than one so a mis-tap on a touch
    /// screen (where the demos have no keyboard at all) does not silently
    /// commit somebody's turn.
    fn click_action(&mut self, layout: &Layout, pos: Pos) {
        let Some(player) = self.sim.awaiting_human() else {
            return;
        };
        let inner = panel_inner(layout.actions);
        if !inner.contains_pos(pos) {
            return;
        }
        let mut actions = self.sim.game().legal_actions(self.sim.state(), player);
        let capacity = usize::from(inner.height());
        let selected = self.selected.min(actions.len().saturating_sub(1));
        let row = usize::from(pos.y.saturating_sub(inner.top()));
        let index = menu_start(capacity, actions.len(), selected) + row;
        if index >= actions.len() {
            return;
        }
        if index == selected {
            let action = actions.swap_remove(index);
            let _ = self.sim.select_human_action(player, action);
            self.selected = 0;
        } else {
            self.selected = index;
        }
    }

    /// Scrolls the log so the scrollbar thumb follows a click or drag at `pos`
    /// on the `bar` track.
    ///
    /// `pos` is clamped into the track first, so a drag that wanders off the
    /// strip pins to its top or bottom rather than being dropped.
    fn drag_log_to(&mut self, bar: Rect, total: usize, visible: usize, pos: Pos) {
        let clamped = Pos::new(
            bar.left(),
            pos.y
                .clamp(bar.top(), bar.bottom().saturating_sub(1).max(bar.top())),
        );
        let Some(start) = offset_for_pos(bar, total, visible, clamped) else {
            return;
        };
        // The bar speaks in lines from the top; the dashboard tracks lines back
        // from the newest, so invert against the same maximum draw_log uses.
        let max_back = total.saturating_sub(visible);
        self.log.offset = max_back.saturating_sub(start);
    }

    /// The furthest the log can scroll back: enough to bring its oldest line to
    /// the top of the panel, and no further.
    fn max_log_scroll(&self) -> usize {
        self.sim
            .log_history()
            .len()
            .saturating_sub(self.log.rows.max(1))
    }

    /// Scrolls the log `lines` further into the past, clamped at the oldest.
    fn scroll_log_back(&mut self, lines: usize) {
        self.log.offset = self
            .log
            .offset
            .saturating_add(lines)
            .min(self.max_log_scroll());
    }

    /// Scrolls the log `lines` back toward the newest line.
    const fn scroll_log_forward(&mut self, lines: usize) {
        self.log.offset = self.log.offset.saturating_sub(lines);
    }

    /// Keeps a scrolled-back log view pinned to the same lines as new entries
    /// arrive during auto-play, then clamps the offset to what currently fits.
    /// Run once per frame before drawing.
    fn anchor_log(&mut self) {
        let len = self.sim.log_history().len();
        if self.log.offset > 0 {
            let grown = len.saturating_sub(self.log.len_seen);
            self.log.offset = self.log.offset.saturating_add(grown);
        }
        self.log.len_seen = len;
        self.log.offset = self.log.offset.min(self.max_log_scroll());
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

    fn handle_setup(&mut self, cursor: usize, events: &[Event]) {
        let seats = self.seats.len();
        let kinds = self.bots.len() + 1;
        let mut cursor = cursor.min(seats.saturating_sub(1));
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
                    self.overlay = Overlay::None;
                    self.rebuild();
                    return;
                }
                KeyCode::Escape => {
                    // Cancel: discard the edits made in the modal.
                    self.seats.clone_from(&self.setup_backup);
                    self.overlay = Overlay::None;
                    return;
                }
                _ => {}
            }
        }
        self.overlay = Overlay::Setup(cursor);
    }

    /// Whether committing `player`'s turn to the action menu would reveal one
    /// human's private view to another. True only in 2+ human pass-and-play
    /// when the board is not already showing `player`'s seat.
    fn reveal_gated(&self, player: PlayerId) -> bool {
        count_humans(&self.seats) >= 2 && self.viewer != Some(player)
    }

    /// While a handoff is pending, wait for Enter to reveal the new seat (or
    /// Esc to quit); every other key is swallowed so nothing leaks early.
    fn handle_handoff(&mut self, seat: PlayerId, events: &[Event]) {
        for key in down_keys(events) {
            match key {
                KeyCode::Enter => {
                    self.viewer = Some(seat);
                    self.overlay = Overlay::None;
                    return;
                }
                KeyCode::Escape => {
                    self.exit = true;
                    return;
                }
                _ => {}
            }
        }
    }

    /// While the help overlay is up, any of `?`/`h`/Esc closes it; the match
    /// stays frozen until then.
    fn handle_help(&mut self, events: &[Event]) {
        for key in down_keys(events) {
            if matches!(key, KeyCode::Char('?' | 'h' | 'H') | KeyCode::Escape) {
                self.overlay = Overlay::None;
                return;
            }
        }
    }

    /// The status-bar summary of whose turn it is: the human (you), the named
    /// AI controlling the active seat, a chance node, or the finished match.
    fn turn_label(&self) -> String {
        if self.sim.is_terminal() {
            return "over".to_owned();
        }
        if let Some(player) = self.sim.awaiting_human() {
            return format!("P{} (you)", player.index());
        }
        match self
            .sim
            .game()
            .active_players(self.sim.state())
            .iter()
            .next()
        {
            Some(player) if player.is_chance() => "chance".to_owned(),
            Some(player) => {
                let name = usize::try_from(player.index())
                    .ok()
                    .and_then(|seat| self.seats.get(seat))
                    .map_or("AI", |kind| self.kind_label(*kind));
                format!("P{} ({name})", player.index())
            }
            None => "over".to_owned(),
        }
    }

    /// Draws one frame and returns the number of visible log rows (so the
    /// caller can clamp scrolling and size a page jump).
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) -> usize {
        term.reset_style();
        let theme = Theme::DARK;
        let layout = Layout::new(term.area());
        let log_rows = if let Overlay::Handoff(seat) = self.overlay {
            // Cover the board so the previous seat's private view is hidden
            // while the device changes hands.
            Self::draw_handoff(term, seat);
            self.log.rows
        } else {
            let view = self.sim.game().view(self.sim.state(), self.viewer);
            let rows = draw_board_stats_log(
                self.sim.game(),
                &view,
                self.sim.log_history(),
                term,
                &layout,
                theme,
                self.log.offset,
            );
            self.draw_actions(term, &layout);
            rows
        };
        self.draw_status(term, &layout);
        match self.overlay {
            Overlay::Setup(cursor) => self.draw_setup(term, cursor),
            Overlay::Help => Self::draw_help(term),
            Overlay::None | Overlay::Handoff(_) => {}
        }
        log_rows
    }

    fn draw_actions<B: Backend>(&self, term: &mut Terminal<B>, layout: &Layout) {
        let theme = Theme::DARK;
        if self.sim.is_terminal() {
            let inner = actions_panel(term, layout.actions, theme, None);
            print_rows(
                term,
                inner,
                0,
                std::iter::once("match over -- r restart, c config".to_owned()),
            );
        } else if let Some(player) = self.sim.awaiting_human() {
            let actions = self.sim.game().legal_actions(self.sim.state(), player);
            let game = self.sim.game();
            let labels: Vec<String> = actions.iter().map(|a| game.format_action(a)).collect();
            let selected = self.selected.min(labels.len().saturating_sub(1));
            // Position/total in the panel title, so a scrolled menu still tells
            // you how many actions there are.
            let inner = actions_panel(
                term,
                layout.actions,
                theme,
                Some((selected + 1, labels.len())),
            );
            draw_menu(term, inner, 0, &labels, selected);
        } else {
            let inner = actions_panel(term, layout.actions, theme, None);
            let label = if !self.auto {
                "(step: press Space)"
            } else if self.paused {
                "(paused: p to resume)"
            } else {
                "(AI thinking...)"
            };
            print_rows(term, inner, 0, std::iter::once(label.to_owned()));
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
        let turn = self.turn_label();
        let text =
            format!(" {mode}{speed}  |  turn: {turn}  |  c config  r reset  ? help  Esc quit");

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

    fn draw_handoff<B: Backend>(term: &mut Terminal<B>, seat: PlayerId) {
        let theme = Theme::DARK;
        let inner = Modal::new(42, 6)
            .theme(theme)
            .title("Pass the device")
            .render(term.area(), term);
        term.reset_style().fg(theme.fg);
        print_rows(
            term,
            inner,
            0,
            [
                format!("Hand the screen to P{}.", seat.index()),
                String::new(),
                "Press Enter when they are ready.".to_owned(),
            ],
        );
        term.reset_style();
    }

    fn draw_help<B: Backend>(term: &mut Terminal<B>) {
        let theme = Theme::DARK;
        let lines = [
            "Space     step one decision",
            "m/Tab     Auto <-> Step",
            "p         pause / resume (Auto)",
            "+ / -     speed",
            "PgUp/PgDn scroll the log (or the wheel)",
            "Home/End  log oldest / newest",
            "c         configure seats",
            "r         restart (new seed)",
            "?         toggle this help",
            "Esc       quit",
            "",
            "Your turn: Up/Down select, Enter play",
            "           or click a row, again to play",
        ];
        #[expect(clippy::cast_possible_truncation, reason = "line count is tiny")]
        let height = lines.len() as u16 + 4;
        let inner = Modal::new(44, height)
            .theme(theme)
            .title("Controls")
            .render(term.area(), term);
        term.reset_style().fg(theme.fg);
        print_rows(term, inner, 0, lines.iter().map(|line| (*line).to_owned()));
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
        // The same rects the previous frame drew (the layout is a pure
        // function of the terminal size), so pointer input hit-tests against
        // what the viewer is actually looking at.
        let layout = Layout::new(term.area());

        match self.overlay {
            // An overlay covers the log, so any drag in progress is over: the
            // release that would end it goes to the overlay, not the strip.
            Overlay::Setup(cursor) => {
                self.log.dragging = false;
                self.handle_setup(cursor, &events);
            }
            Overlay::Help => {
                self.log.dragging = false;
                self.handle_help(&events);
            }
            Overlay::Handoff(seat) => {
                self.log.dragging = false;
                self.handle_handoff(seat, &events);
            }
            Overlay::None => {
                self.handle_controls(&events);
                self.handle_mouse(&layout, &events);
                // handle_controls may have opened an overlay or set exit; only
                // touch the match if the live dashboard is still in front.
                if !self.exit && matches!(self.overlay, Overlay::None) && !self.sim.is_terminal() {
                    match self.sim.awaiting_human() {
                        Some(player) if self.reveal_gated(player) => {
                            // Blank the screen and wait for the device to change
                            // hands before revealing this seat's view.
                            self.overlay = Overlay::Handoff(player);
                        }
                        Some(player) => self.handle_action_menu(player, &events),
                        None => {
                            if self.auto && !self.paused {
                                self.tick_ai(frame);
                            }
                        }
                    }
                }
            }
        }

        // Keep a scrolled-back log anchored as new lines arrive, then draw and
        // record how many log rows were visible for the next frame's clamping.
        self.anchor_log();
        self.log.rows = self.draw(term).max(1);
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use retroglyph_core::backend::Headless;
    use retroglyph_core::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use retroglyph_core::grid::{Pos, Rect};
    use retroglyph_core::{Backend, Flow, Frame, Terminal, step};
    use turnbase::{ActivePlayers, Game, PlayerId};

    use super::{SeatKind, SessionApp, build_sim, count_humans, standard_bots, viewer_for};
    use crate::PrintableGame;
    use crate::dashboard::{Layout, log_geometry, log_start, panel_inner};

    /// A two-seat game that takes one action per seat then ends. Enough state
    /// to exercise seat scheduling, human turns, and rebuilds; the view is the
    /// public move count (no hidden info needed for these tests).
    #[derive(Clone)]
    struct TwoSeat;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Play;

    impl Game for TwoSeat {
        type State = u32;
        type Action = Play;
        type View = u32;

        fn new_initial_state(&self, _seed: u64) -> u32 {
            0
        }
        fn num_players(&self) -> usize {
            2
        }
        fn active_players(&self, state: &u32) -> ActivePlayers {
            if *state >= 2 {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::new(*state % 2))
            }
        }
        fn legal_actions(&self, state: &u32, _player: PlayerId) -> Vec<Play> {
            if *state >= 2 { Vec::new() } else { vec![Play] }
        }
        fn apply(&self, state: &mut u32, _player: PlayerId, _action: Play) {
            *state += 1;
        }
        fn is_terminal(&self, state: &u32) -> bool {
            *state >= 2
        }
        fn reward(&self, _state: &u32, _player: PlayerId) -> f64 {
            0.0
        }
        fn view(&self, state: &u32, _viewer: Option<PlayerId>) -> u32 {
            *state
        }
    }

    impl PrintableGame for TwoSeat {
        fn draw_viewport<B: Backend>(&self, _view: &u32, _term: &mut Terminal<B>, _area: Rect) {}
        fn get_stats(&self, _view: &u32) -> Vec<(String, String)> {
            Vec::new()
        }
        fn format_action(&self, _action: &Play) -> String {
            "play".to_owned()
        }
    }

    /// Drives `app` for up to `frames` frames on a headless terminal, pressing
    /// Enter each frame when `enter` is set, and reports whether the match
    /// finished.
    fn drive(mut app: SessionApp<TwoSeat>, enter: bool, frames: u64) -> bool {
        let mut term = Terminal::new(Headless::new(60, 20));
        for frame in 0..frames {
            if enter {
                term.backend_mut().push_event(Event::Key(KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                )));
            }
            let ctx = Frame {
                delta: Duration::from_millis(250),
                frame,
            };
            if step(&mut term, &mut app, &ctx) == Flow::Exit {
                break;
            }
            if app.is_terminal() {
                return true;
            }
        }
        app.is_terminal()
    }

    /// A one-seat game that takes `MANY_MOVES` turns, so its log outgrows any
    /// log panel and the scroll paths have something to scroll.
    #[derive(Clone)]
    struct LongGame;

    const MANY_MOVES: u32 = 200;

    impl Game for LongGame {
        type State = u32;
        type Action = Play;
        type View = u32;

        fn new_initial_state(&self, _seed: u64) -> u32 {
            0
        }
        fn num_players(&self) -> usize {
            1
        }
        fn active_players(&self, state: &u32) -> ActivePlayers {
            if *state >= MANY_MOVES {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::new(0))
            }
        }
        fn legal_actions(&self, state: &u32, _player: PlayerId) -> Vec<Play> {
            if *state >= MANY_MOVES {
                Vec::new()
            } else {
                vec![Play]
            }
        }
        fn apply(&self, state: &mut u32, _player: PlayerId, _action: Play) {
            *state += 1;
        }
        fn is_terminal(&self, state: &u32) -> bool {
            *state >= MANY_MOVES
        }
        fn reward(&self, _state: &u32, _player: PlayerId) -> f64 {
            0.0
        }
        fn view(&self, state: &u32, _viewer: Option<PlayerId>) -> u32 {
            *state
        }
    }

    impl PrintableGame for LongGame {
        fn draw_viewport<B: Backend>(&self, _view: &u32, _term: &mut Terminal<B>, _area: Rect) {}
        fn get_stats(&self, _view: &u32) -> Vec<(String, String)> {
            Vec::new()
        }
        fn format_action(&self, _action: &Play) -> String {
            "play".to_owned()
        }
    }

    const TEST_COLS: u16 = 60;
    const TEST_ROWS: u16 = 20;

    /// A session on a filled-up log, plus the terminal it was drawn on, ready
    /// for the pointer tests to poke at the log strip.
    fn scrolled_session() -> (SessionApp<LongGame>, Terminal<Headless>) {
        let mut app = SessionApp::new(LongGame, standard_bots(), 7);
        let mut term = Terminal::new(Headless::new(TEST_COLS, TEST_ROWS));
        // Auto mode steps once per SPEEDS[speed]; a generous delta per frame
        // fills the log in a handful of frames.
        for frame in 0..40 {
            let ctx = Frame {
                delta: Duration::from_secs(1),
                frame,
            };
            let _ = step(&mut term, &mut app, &ctx);
        }
        assert!(
            app.sim.log_history().len() > app.log.rows,
            "the log must outgrow the panel for the scroll tests to mean anything"
        );
        (app, term)
    }

    /// Feeds `app` one mouse event and runs a frame.
    fn pointer(
        app: &mut SessionApp<LongGame>,
        term: &mut Terminal<Headless>,
        kind: MouseEventKind,
        position: Pos,
    ) {
        term.backend_mut().push_event(Event::Mouse(MouseEvent {
            kind,
            position,
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let ctx = Frame {
            delta: Duration::ZERO,
            frame: 0,
        };
        let _ = step(term, app, &ctx);
    }

    #[test]
    fn the_wheel_scrolls_the_log_only_over_the_log() {
        let layout = Layout::new(Rect::new(0, 0, TEST_COLS, TEST_ROWS));
        let inside = Pos::new(layout.log.left() + 2, layout.log.top() + 1);
        let elsewhere = Pos::new(layout.viewport.left() + 2, layout.viewport.top() + 1);

        let (mut app, mut term) = scrolled_session();
        pointer(&mut app, &mut term, MouseEventKind::ScrollUp, inside);
        assert_eq!(
            app.log.offset,
            super::WHEEL_LINES,
            "a wheel notch over the log should scroll it back"
        );
        pointer(&mut app, &mut term, MouseEventKind::ScrollDown, inside);
        assert_eq!(app.log.offset, 0, "scrolling forward returns to the tail");

        pointer(&mut app, &mut term, MouseEventKind::ScrollUp, elsewhere);
        assert_eq!(
            app.log.offset, 0,
            "the wheel elsewhere on the dashboard must not scroll the log"
        );
    }

    #[test]
    fn dragging_the_scrollbar_moves_through_the_log() {
        let layout = Layout::new(Rect::new(0, 0, TEST_COLS, TEST_ROWS));
        let (mut app, mut term) = scrolled_session();
        let total = app.sim.log_history().len();
        let geometry = log_geometry(layout.log, total);
        let bar = geometry.bar.expect("a filled log should show a scrollbar");
        let max_back = total.saturating_sub(geometry.visible);

        // Grabbing the top of the track jumps to the oldest line...
        pointer(
            &mut app,
            &mut term,
            MouseEventKind::Down(MouseButton::Left),
            Pos::new(bar.left(), bar.top()),
        );
        assert_eq!(app.log.offset, max_back, "the top of the track is oldest");
        assert_eq!(
            log_start(total, geometry.visible, app.log.offset),
            0,
            "which is line 0 in the panel's own coordinates"
        );

        // ...and dragging past the bottom of the track pins to the newest,
        // even though the pointer has left the strip.
        pointer(
            &mut app,
            &mut term,
            MouseEventKind::Moved,
            Pos::new(bar.left(), TEST_ROWS - 1),
        );
        assert_eq!(app.log.offset, 0, "the bottom of the track is newest");

        // Releasing ends the drag: later moves are just hovering.
        pointer(
            &mut app,
            &mut term,
            MouseEventKind::Up(MouseButton::Left),
            Pos::new(bar.left(), bar.bottom() - 1),
        );
        pointer(
            &mut app,
            &mut term,
            MouseEventKind::Moved,
            Pos::new(bar.left(), bar.top()),
        );
        assert_eq!(
            app.log.offset, 0,
            "a hover after the release scrolls nothing"
        );
    }

    #[test]
    fn clicking_an_action_row_selects_then_plays_it() {
        let layout = Layout::new(Rect::new(0, 0, TEST_COLS, TEST_ROWS));
        let actions = panel_inner(layout.actions);
        // A human seat, so the actions panel holds a menu rather than a status
        // line; TwoSeat offers exactly one action per turn.
        let mut app = SessionApp::new(TwoSeat, standard_bots(), 11).with_human_seat(0);
        let mut term = Terminal::new(Headless::new(TEST_COLS, TEST_ROWS));
        let ctx = Frame {
            delta: Duration::ZERO,
            frame: 0,
        };
        let _ = step(&mut term, &mut app, &ctx);
        assert_eq!(app.sim.state(), &0, "nothing has been played yet");

        let row = Pos::new(actions.left() + 2, actions.top());
        let click = |app: &mut SessionApp<TwoSeat>, term: &mut Terminal<Headless>| {
            term.backend_mut().push_event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                position: row,
                pixel_position: None,
                modifiers: KeyModifiers::NONE,
            }));
            let _ = step(term, app, &ctx);
        };

        // The first click lands on the already-selected row, which is the
        // confirm: one click is enough when there is a single legal action.
        click(&mut app, &mut term);
        assert_eq!(
            app.sim.state(),
            &1,
            "clicking the selected action should play it"
        );
    }

    #[test]
    fn a_click_outside_the_actions_panel_plays_nothing() {
        let layout = Layout::new(Rect::new(0, 0, TEST_COLS, TEST_ROWS));
        let mut app = SessionApp::new(TwoSeat, standard_bots(), 12).with_human_seat(0);
        let mut term = Terminal::new(Headless::new(TEST_COLS, TEST_ROWS));
        let ctx = Frame {
            delta: Duration::ZERO,
            frame: 0,
        };
        term.backend_mut().push_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(layout.viewport.left() + 1, layout.viewport.top() + 1),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }));
        let _ = step(&mut term, &mut app, &ctx);
        assert_eq!(app.sim.state(), &0, "the board is not a menu");
    }

    #[test]
    fn viewer_follows_seat_config() {
        let bots = standard_bots::<TwoSeat>();
        // All AI: a neutral spectator (no seat's private view).
        let all_ai = [SeatKind::Ai(0), SeatKind::Ai(0)];
        let sim = build_sim(&TwoSeat, &all_ai, &bots, 0);
        assert_eq!(viewer_for(&all_ai, &sim), None);
        // One human: fixed at that seat.
        let one = [SeatKind::Human, SeatKind::Ai(0)];
        let sim = build_sim(&TwoSeat, &one, &bots, 0);
        assert_eq!(viewer_for(&one, &sim), Some(PlayerId::new(0)));
        // Two humans: spectator until a handoff reveals the acting seat.
        let two = [SeatKind::Human, SeatKind::Human];
        let sim = build_sim(&TwoSeat, &two, &bots, 0);
        assert_eq!(viewer_for(&two, &sim), None);
        assert_eq!(count_humans(&two), 2);
    }

    #[test]
    fn all_ai_auto_plays_to_the_end() {
        // The demo case: no seats human, Auto mode, no input needed.
        let app = SessionApp::new(TwoSeat, standard_bots(), 1);
        assert!(
            drive(app, false, 60),
            "an all-AI match should finish on its own"
        );
    }

    #[test]
    fn one_human_needs_no_handoff() {
        // A single human seat plays via the menu (Enter confirms); the AI seat
        // auto-steps. No handoff gate, so Enter alone drives it to the end.
        let app = SessionApp::new(TwoSeat, standard_bots(), 2).with_human_seat(0);
        assert!(drive(app, true, 60), "one human + Enter should finish");
    }

    #[test]
    fn two_humans_block_on_a_handoff() {
        // With two human seats, no seat is revealed until an Enter passes the
        // device: with no input the match must not advance at all.
        let app = SessionApp::new(TwoSeat, standard_bots(), 3)
            .with_human_seat(0)
            .with_human_seat(1);
        assert!(
            !drive(app, false, 60),
            "a 2-human match must stall on the handoff without input"
        );
        // Feeding Enter clears each handoff and confirms each seat's move.
        let app = SessionApp::new(TwoSeat, standard_bots(), 3)
            .with_human_seat(0)
            .with_human_seat(1);
        assert!(
            drive(app, true, 60),
            "Enter should pass the device and play both seats to the end"
        );
    }
}
