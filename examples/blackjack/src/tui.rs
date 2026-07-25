//! A bespoke retroglyph terminal UI for Blackjack.
//!
//! Every other Tier-1 example implements `turnbase_simulator::PrintableGame`
//! and defers to the shared four-panel dashboard. Blackjack instead ships its
//! own UI, to show a game can: this drives a [`turnbase_match::Simulator`]
//! through a hand-written [`retroglyph_core::App`] with a card-table layout and
//! blackjack-flavored keys (press `H` to hit, `S` to stand). It is wired in via
//! [`turnbase_cli::run_with_play`], so the headless `new`/`query`/`act` and
//! `self-play` commands stay shared while only `play` is custom.

use std::time::Duration;

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{AnsiColor, App, Backend, Color, Flow, Frame, Terminal};
use turnbase::Game;
use turnbase_match::Simulator;

use crate::{Action, Blackjack, BlackjackView, Card, Outcome, PLAYER, Phase, best_total, is_soft};

// The crossterm `play` entry point and its helpers only exist in the native
// `ui` build; the App itself (`BlackjackTui`) is backend-generic and also
// powers the wasm demo via the `app` feature.
#[cfg(feature = "ui")]
use crate::DEALER;
#[cfg(feature = "ui")]
use std::collections::HashMap;
#[cfg(feature = "ui")]
use std::process::ExitCode;
#[cfg(feature = "ui")]
use turnbase_bots::Random;
#[cfg(feature = "ui")]
use turnbase_cli::PlayArgs;
#[cfg(feature = "ui")]
use turnbase_match::PlayerAgent;

/// How long a non-human step (a chance deal or the scripted dealer) waits, so
/// cards land one at a time instead of appearing all at once.
const TICK: Duration = Duration::from_millis(600);

/// The `play` handler for [`turnbase_cli::run_with_play`]: seat 0 is you, seat
/// 1 the scripted dealer, chance deals the shoe, and the whole match renders
/// through the bespoke `BlackjackTui`.
#[cfg(feature = "ui")]
#[must_use]
pub fn play(game: Blackjack, args: &PlayArgs) -> ExitCode {
    let seed = args.seed().unwrap_or_else(random_seed);
    // A builder (not a one-off Simulator) so the dashboard's reset key can
    // rebuild the match: seat 0 is you, seat 1 the scripted dealer.
    let builder = move |seed: u64| {
        let mut agents = HashMap::new();
        agents.insert(PLAYER, PlayerAgent::Human);
        // The dealer's `legal_actions` is a singleton, so any agent plays its
        // script; a Random is the simplest way to fill the seat.
        agents.insert(
            DEALER,
            PlayerAgent::Ai(Box::new(Random::new(seed ^ 0x5EED_D00D))),
        );
        Simulator::new(game, seed, agents)
    };
    match retroglyph_crossterm::Crossterm::run(BlackjackTui::with_builder(seed, builder)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The bespoke dashboard: a [`Simulator`] plus a clock that paces non-human
/// steps so the table animates.
///
/// Public and backend-generic so the wasm demo can build one over a browser
/// terminal backend, not just the native crossterm `play`.
pub struct BlackjackTui {
    sim: Simulator<Blackjack>,
    elapsed: Duration,
    auto: bool,
    paused: bool,
    seed: u64,
    // Some(builder) enables reset (`r`): the match is rebuilt for the next
    // seed. `new` leaves this None (reset is a no-op), so a headless test can
    // still wrap a one-off Simulator.
    builder: Option<Box<dyn Fn(u64) -> Simulator<Blackjack>>>,
}

impl BlackjackTui {
    /// Wraps a ready-to-play [`Simulator`] (seat 0 the player, seat 1 the
    /// dealer) in the dashboard. Reset (`r`) is disabled; use
    /// [`BlackjackTui::with_builder`] to enable it.
    #[must_use]
    pub const fn new(sim: Simulator<Blackjack>) -> Self {
        Self {
            sim,
            elapsed: Duration::ZERO,
            auto: true,
            paused: false,
            seed: 0,
            builder: None,
        }
    }

    /// Wraps a fresh match built from `seed` by `builder`, keeping `builder`
    /// so reset (`r`) can rebuild for the next seed.
    #[must_use]
    pub fn with_builder(
        seed: u64,
        builder: impl Fn(u64) -> Simulator<Blackjack> + 'static,
    ) -> Self {
        Self {
            sim: builder(seed),
            elapsed: Duration::ZERO,
            auto: true,
            paused: false,
            seed,
            builder: Some(Box::new(builder)),
        }
    }

    /// Whether the wrapped match has ended.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.sim.is_terminal()
    }

    /// Rebuilds the match for a fresh seed, if a builder was supplied.
    fn reset(&mut self) {
        if let Some(builder) = &self.builder {
            self.seed = self.seed.wrapping_add(1);
            self.sim = builder(self.seed);
            self.elapsed = Duration::ZERO;
            self.paused = false;
        }
    }

    /// Advances one non-human step (a chance deal or the scripted dealer);
    /// never acts for the human seat. The only way to progress in Step mode,
    /// and a manual nudge in Auto.
    fn step_once(&mut self) {
        if !self.sim.is_terminal() && self.sim.awaiting_human().is_none() {
            let _ = self.sim.step();
            self.elapsed = Duration::ZERO;
        }
    }

    /// Applies the player's Hit/Stand key, ignoring anything illegal.
    fn handle_keys(&mut self, events: &[Event]) {
        for event in events {
            let Event::Key(key) = event else { continue };
            if !key.is_down() {
                continue;
            }
            let action = match key.code {
                KeyCode::Char('h' | 'H') => Action::Hit,
                KeyCode::Char('s' | 'S') => Action::Stand,
                _ => continue,
            };
            if self.sim.game().is_legal(self.sim.state(), PLAYER, &action) {
                let _ = self.sim.select_human_action(PLAYER, action);
            }
        }
    }

    /// Renders the whole table from the player's (redacted) view.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let view = self.sim.game().view(self.sim.state(), Some(PLAYER));
        let left = term.area().left().saturating_add(2);
        let mut y = term.area().top().saturating_add(1);

        term.fg(Color::Ansi(AnsiColor::BrightWhite));
        term.print(left, y, "B L A C K J A C K");
        term.reset_style();
        y = y.saturating_add(2);

        term.print(
            left,
            y,
            &format!(
                "hand {} of {}      you {} - {} dealer",
                (view.round + 1).min(view.hands),
                view.hands,
                view.player_wins,
                view.dealer_wins,
            ),
        );
        y = y.saturating_add(2);

        term.fg(Color::Ansi(AnsiColor::BrightRed));
        term.print(left, y, "dealer");
        term.reset_style();
        term.print(left.saturating_add(9), y, &dealer_row(&view));
        y = y.saturating_add(1);

        term.fg(Color::Ansi(AnsiColor::BrightGreen));
        term.print(left, y, "you");
        term.reset_style();
        term.print(left.saturating_add(9), y, &player_row(&view));
        y = y.saturating_add(2);

        self.draw_prompt(term, &view, left, y);
    }

    /// Draws the status line and controls below the table.
    fn draw_prompt<B: Backend>(
        &self,
        term: &mut Terminal<B>,
        view: &BlackjackView,
        left: u16,
        y: u16,
    ) {
        if matches!(view.phase, Phase::Done) {
            term.fg(Color::Ansi(AnsiColor::BrightYellow));
            term.print(left, y, match_result(view));
            term.reset_style();
            term.print(left, y.saturating_add(2), "r restart   Enter/Esc to exit");
            return;
        }
        if let Some(outcome) = view.outcome {
            term.print(left, y, &format!("last hand: {}", outcome_text(outcome)));
        }
        let prompt_y = y.saturating_add(1);
        if self.sim.awaiting_human() == Some(PLAYER) {
            term.fg(Color::Ansi(AnsiColor::BrightCyan));
            term.print(left, prompt_y, "your move:  [H]it   [S]tand");
            term.reset_style();
        } else {
            let mode = if self.auto {
                if self.paused {
                    "paused"
                } else {
                    "dealer is playing..."
                }
            } else {
                "step: press Space"
            };
            term.print(left, prompt_y, mode);
        }
        term.print(
            left,
            prompt_y.saturating_add(2),
            "Space step  m mode  p pause  r reset  Esc quit",
        );
    }
}

impl<B: Backend> App<B> for BlackjackTui {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        let events: Vec<Event> = term.drain_events().collect();
        if pressed(&events, KeyCode::Escape) {
            return Flow::Exit;
        }

        // Run controls, distinct from the H/S play keys: r reset, m Auto/Step,
        // p pause, Space step.
        for event in &events {
            let Event::Key(key) = event else { continue };
            if !key.is_down() {
                continue;
            }
            match key.code {
                KeyCode::Char('r' | 'R') => self.reset(),
                KeyCode::Char('m' | 'M') | KeyCode::Tab => {
                    self.auto = !self.auto;
                    self.paused = false;
                    self.elapsed = Duration::ZERO;
                }
                KeyCode::Char('p' | 'P') => {
                    if self.auto {
                        self.paused = !self.paused;
                    }
                }
                KeyCode::Char(' ') => self.step_once(),
                _ => {}
            }
        }

        if self.sim.is_terminal() {
            if pressed(&events, KeyCode::Enter) {
                return Flow::Exit;
            }
        } else if self.sim.awaiting_human() == Some(PLAYER) {
            self.handle_keys(&events);
        } else if self.auto && !self.paused {
            // Pace the scripted dealer and chance deals so the table animates.
            self.elapsed = self.elapsed.saturating_add(frame.delta);
            if self.elapsed >= TICK {
                self.elapsed = Duration::ZERO;
                let _ = self.sim.step();
            }
        }

        self.draw(term);
        let _ = term.present();
        Flow::Continue
    }
}

/// Returns true if `events` holds a press of `code`.
fn pressed(events: &[Event], code: KeyCode) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::Key(k) if k.is_down() && k.code == code))
}

/// Renders a hand as `[A] [10] [K]`, or `--` if empty.
fn cards(hand: &[Card]) -> String {
    if hand.is_empty() {
        return "--".to_owned();
    }
    hand.iter()
        .map(|c| format!("[{c}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The dealer's row: up-cards plus `[?]` for the hidden hole, and a total.
fn dealer_row(view: &BlackjackView) -> String {
    if view.hole_revealed {
        format!(
            "{}   ({})",
            cards(&view.dealer_hand),
            total_text(&view.dealer_hand)
        )
    } else {
        // The hole is redacted out of the player's view, so show it as `[?]`
        // and total only the up-cards.
        format!(
            "{} [?]   (showing {})",
            cards(&view.dealer_hand),
            best_total(&view.dealer_hand)
        )
    }
}

/// The player's row: cards plus a total.
fn player_row(view: &BlackjackView) -> String {
    format!(
        "{}   ({})",
        cards(&view.player_hand),
        total_text(&view.player_hand)
    )
}

/// A total like `18`, `soft 17`, or `bust 24`.
fn total_text(hand: &[Card]) -> String {
    let total = best_total(hand);
    if total > 21 {
        format!("bust {total}")
    } else if is_soft(hand) {
        format!("soft {total}")
    } else {
        total.to_string()
    }
}

/// One-line description of a single hand's outcome.
const fn outcome_text(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::PlayerWin => "you won",
        Outcome::DealerWin => "dealer won",
        Outcome::Push => "push",
    }
}

/// Declares the match winner from the final hand tally.
fn match_result(view: &BlackjackView) -> &'static str {
    match view.player_wins.cmp(&view.dealer_wins) {
        std::cmp::Ordering::Greater => "match over: you win!",
        std::cmp::Ordering::Less => "match over: dealer wins",
        std::cmp::Ordering::Equal => "match over: a draw",
    }
}

/// A process-random seed, matching the CLI's unseeded behavior.
#[cfg(feature = "ui")]
fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use retroglyph_core::backend::Headless;
    use retroglyph_core::{Flow, Frame, Terminal, step};
    use turnbase_bots::Random;
    use turnbase_match::{PlayerAgent, Simulator};

    use super::BlackjackTui;
    use crate::{Blackjack, DEALER, PLAYER};

    #[test]
    fn tui_renders_a_full_match_headless() {
        // Both seats are bots so the App drives itself without waiting on a
        // human, exercising every draw path (dealing, player turn, dealer
        // turn, hand transitions, match over) against retroglyph's headless
        // backend without a real terminal.
        let mut agents = HashMap::new();
        agents.insert(PLAYER, PlayerAgent::Ai(Box::new(Random::new(1))));
        agents.insert(DEALER, PlayerAgent::Ai(Box::new(Random::new(2))));
        let sim = Simulator::new(Blackjack::new(3), 7, agents);
        let mut tui = BlackjackTui::new(sim);
        let mut term = Terminal::new(Headless::new(80, 24));

        let mut reached_terminal = false;
        for frame in 0..5000u64 {
            // A delta past `TICK` so each frame advances one non-human step.
            let ctx = Frame {
                delta: Duration::from_millis(700),
                frame,
            };
            if step(&mut term, &mut tui, &ctx) == Flow::Exit {
                break;
            }
            if tui.sim.is_terminal() {
                reached_terminal = true;
                break;
            }
        }
        assert!(
            reached_terminal,
            "a bot-vs-bot match should reach a terminal state within the frame budget"
        );
    }
}
