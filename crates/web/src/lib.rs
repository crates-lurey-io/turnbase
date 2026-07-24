//! WASM entry point for the in-browser demos of the reference games.
//!
//! Each demo page loads this one module and constructs a [`Demo`] for its game
//! by name. The demo runs an all-bot self-play match, so it animates with no
//! input, through the exact same App the native terminal client uses:
//! [`SimulationRunner`] for the `PrintableGame` games, `BlackjackTui` for
//! blackjack. Rendering goes into a [`TerminalWasm`] whose ANSI output the
//! page writes to an xterm.js terminal; when a match ends the demo restarts
//! with a fresh seed so the gallery stays live.
//!
//! Everything here is backend-generic and crossterm-free, which is why the
//! game crates are pulled with only their wasm-safe rendering feature
//! (`printable` / `app`) and `turnbase-simulator` with its `crossterm`
//! feature off.

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use retroglyph_core::event::Event;
use retroglyph_core::{App, Flow, Frame, Terminal};
use retroglyph_terminal_wasm::{TerminalWasm, decode_key_event};
use turnbase::{Game, PlayerId};
use turnbase_bots::RandomBot;
use turnbase_match::{PlayerAgent, Simulator};
use turnbase_simulator::{PrintableGame, SimulationRunner};
use wasm_bindgen::prelude::wasm_bindgen;

use blackjack::Blackjack;
use blackjack::tui::BlackjackTui;
use coup::Coup;
use minion_battle::MinionBattle;
use risk::Risk;
use woodland::Woodland;

/// How long an AI seat waits between moves, so the demo is watchable.
const AI_TICK: Duration = Duration::from_millis(450);
/// How long to linger on a finished match before restarting it.
const RESTART_AFTER: Duration = Duration::from_millis(2500);

/// A running demo App, one variant per reference game.
enum Runner {
    Coup(SimulationRunner<Coup>),
    Risk(SimulationRunner<Risk>),
    Minion(SimulationRunner<MinionBattle>),
    Woodland(SimulationRunner<Woodland>),
    Blackjack(BlackjackTui),
}

impl Runner {
    fn update(&mut self, term: &mut Terminal<TerminalWasm>, frame: &Frame) -> Flow {
        match self {
            Self::Coup(r) => r.update(term, frame),
            Self::Risk(r) => r.update(term, frame),
            Self::Minion(r) => r.update(term, frame),
            Self::Woodland(r) => r.update(term, frame),
            Self::Blackjack(r) => r.update(term, frame),
        }
    }

    fn is_terminal(&self) -> bool {
        match self {
            Self::Coup(r) => r.is_terminal(),
            Self::Risk(r) => r.is_terminal(),
            Self::Minion(r) => r.is_terminal(),
            Self::Woodland(r) => r.is_terminal(),
            Self::Blackjack(r) => r.is_terminal(),
        }
    }
}

/// Builds an all-bot [`SimulationRunner`] for a `PrintableGame`.
fn runner<G>(game: G, seed: u64) -> SimulationRunner<G>
where
    G: PrintableGame,
    G::Action: Debug,
{
    SimulationRunner::new(all_bots(game, seed), AI_TICK)
}

/// Builds a [`Simulator`] with every seat driven by a per-seat-seeded bot.
fn all_bots<G: Game>(game: G, seed: u64) -> Simulator<G> {
    let mut agents = HashMap::new();
    for seat in 0..game.num_players() {
        let index = u32::try_from(seat).expect("seat index fits in u32");
        let id = PlayerId::new(index);
        let bot = RandomBot::new(seed ^ (u64::from(index) + 1));
        agents.insert(id, PlayerAgent::Ai(Box::new(bot)));
    }
    Simulator::new(game, seed, agents)
}

/// Builds the [`Runner`] for `game`, defaulting unknown names to Coup.
fn build(game: &str, seed: u64) -> Runner {
    match game {
        "risk" => Runner::Risk(runner(Risk::new(3), seed)),
        "minion_battle" => Runner::Minion(runner(MinionBattle, seed)),
        "woodland" => Runner::Woodland(runner(Woodland, seed)),
        "blackjack" => Runner::Blackjack(BlackjackTui::new(all_bots(Blackjack::default(), seed))),
        _ => Runner::Coup(runner(Coup::new(4), seed)),
    }
}

/// A single in-browser demo: a terminal backend plus the game's App, driven
/// one animation frame at a time from JS.
#[wasm_bindgen]
pub struct Demo {
    term: Terminal<TerminalWasm>,
    runner: Runner,
    game: String,
    seed: u64,
    idle: Duration,
    frame: u64,
}

#[wasm_bindgen]
impl Demo {
    /// Creates a demo of `game` (one of `coup`, `risk`, `minion_battle`,
    /// `woodland`, `blackjack`; unknown names fall back to Coup) on a grid of
    /// `cols` by `rows` cells, seeded from `seed`.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(game: &str, cols: u16, rows: u16, seed: u32) -> Self {
        let seed = u64::from(seed);
        Self {
            term: Terminal::new(TerminalWasm::new(cols, rows)),
            runner: build(game, seed),
            game: game.to_owned(),
            seed,
            idle: Duration::ZERO,
            frame: 0,
        }
    }

    /// Advances the match by `delta_ms` milliseconds and returns the ANSI
    /// bytes to write to the terminal emulator (empty if nothing changed).
    #[must_use]
    pub fn tick(&mut self, delta_ms: f64) -> String {
        let delta = Duration::from_secs_f64(delta_ms.max(0.0) / 1000.0);
        let frame = Frame {
            delta,
            frame: self.frame,
        };
        self.frame = self.frame.wrapping_add(1);
        let _ = self.runner.update(&mut self.term, &frame);

        if self.runner.is_terminal() {
            self.idle = self.idle.saturating_add(delta);
            if self.idle >= RESTART_AFTER {
                self.seed = self.seed.wrapping_add(1);
                self.runner = build(&self.game, self.seed);
                self.idle = Duration::ZERO;
            }
        } else {
            self.idle = Duration::ZERO;
        }

        self.term.backend_mut().take_output()
    }

    /// Forwards a key from the page (see `decode_key_event`'s encoding).
    pub fn key(&mut self, code: u32, mods: u8) {
        if let Some(event) = decode_key_event(code, mods) {
            self.term.backend_mut().push_event(Event::Key(event));
        }
    }

    /// Resizes the grid to match the browser terminal.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(cols, rows);
    }
}
