//! Shared harness for the in-browser WASM demos of the reference games.
//!
//! Not published (`publish = false`). Each game gets a one-file entry under
//! `examples/` that names a concrete [`Demo`] and closes with
//! [`demo_entry!`]; building that example for `wasm32` (`--example <game>
//! --features <game>`) produces a self-contained module whose
//! `wasm-bindgen` exports a page drives. There is no runtime dispatch and no
//! crate that imports every game: one wasm artifact contains exactly one game,
//! the same way `retroglyph-examples` builds one `[[example]]` at a time.
//!
//! A demo runs an all-bot self-play match (see [`all_bots`]) through the exact
//! same App the native client uses -- the interactive `SessionApp` for the
//! `PrintableGame` dashboard games via [`dashboard`], or a game's own App
//! (e.g. blackjack's `BlackjackTui`) -- so it animates with no input and looks
//! identical to the terminal client. When the match ends, [`WasmDemo`]
//! restarts it with a fresh seed so a gallery page stays live.

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use retroglyph_core::backend::Input;
use retroglyph_core::{App, Terminal};
use retroglyph_terminal_wasm::TerminalWasm;
use turnbase::{Game, PlayerId};
use turnbase_bots::Random;
use turnbase_match::{PlayerAgent, Simulator};
use turnbase_simulator::{BotOption, PrintableGame, SessionApp};
use web_time::Instant;

/// How long to linger on a finished match before restarting it with a fresh
/// seed.
pub const RESTART_AFTER: Duration = Duration::from_millis(2500);

/// A game's browser demo: how to build its App from a seed, and how to tell
/// when its match has ended so the harness can restart it.
///
/// Implemented once per game by a zero-sized marker type in that game's
/// `examples/` entry file, then handed to [`demo_entry!`], which generates the
/// `wasm-bindgen` FFI for it. Mirrors `retroglyph-examples`'s `Example`
/// trait: the concrete game type is pinned here, in the entry, so the FFI
/// symbols `wasm-bindgen` needs can be statically named.
///
/// Requires [`Default`] only so the host-side stub `main` that [`demo_entry!`]
/// emits can construct the marker (its real use, the `wasm-bindgen` FFI, is
/// `wasm32`-only, so without this the marker would be dead code on the host
/// where the workspace lint/test jobs run). A `#[derive(Default)]` on the
/// unit marker struct is all it takes.
pub trait Demo: Default {
    /// The App this demo drives against a [`TerminalWasm`] terminal.
    type App: App<TerminalWasm>;

    /// Builds a fresh all-bot match App seeded from `seed`.
    fn build(seed: u64) -> Self::App;

    /// Returns whether the wrapped match has reached a terminal state, so
    /// [`WasmDemo`] knows when to start the restart countdown.
    fn is_over(app: &Self::App) -> bool;
}

/// Builds an interactive [`SessionApp`] for a `PrintableGame`.
///
/// Runs all-bot auto-play out of the box; the viewer can press `c` to
/// configure seats, pick an AI type, take a seat, step, or reset. The
/// dashboard [`Demo::build`] for every `PrintableGame` game; `bots` is the
/// per-game set of AI types the setup modal offers.
pub fn dashboard<G>(game: G, bots: Vec<BotOption<G>>, seed: u64) -> SessionApp<G>
where
    G: PrintableGame + Clone,
    G::Action: Debug,
{
    SessionApp::new(game, bots, seed)
}

/// Builds a [`Simulator`] for `game` with every seat driven by its own
/// per-seat-seeded [`Random`], so a demo plays itself.
///
/// # Panics
///
/// Panics if `game.num_players()` exceeds `u32::MAX` (no real game comes
/// remotely close; seats are addressed by a `u32` [`PlayerId`]).
#[must_use]
pub fn all_bots<G: Game>(game: G, seed: u64) -> Simulator<G> {
    let mut agents = HashMap::new();
    for seat in 0..game.num_players() {
        let index = u32::try_from(seat).expect("seat index fits in u32");
        let id = PlayerId::new(index);
        let bot = Random::new(seed ^ (u64::from(index) + 1));
        agents.insert(id, PlayerAgent::Ai(Box::new(bot)));
    }
    Simulator::new(game, seed, agents)
}

/// Drives a [`Demo`]'s App against a `Terminal<TerminalWasm>`, one animation
/// frame at a time.
///
/// Restarts the match with a fresh seed once it has been finished for
/// [`RESTART_AFTER`], so a gallery page never goes stale. The per-game
/// `wasm-bindgen` entry ([`demo_entry!`]) owns one of these in a thread-local
/// and forwards the browser's `init`/`tick`/`key`/`resize` calls to it.
pub struct WasmDemo<D: Demo> {
    term: Terminal<TerminalWasm>,
    app: D::App,
    seed: u64,
    idle: Duration,
    last: Instant,
    frame: u64,
}

impl<D: Demo> WasmDemo<D> {
    /// Builds a demo of `D` on a grid of `cols` by `rows` cells, seeded from
    /// `seed`.
    #[must_use]
    pub fn new(cols: u16, rows: u16, seed: u64) -> Self {
        Self {
            term: Terminal::new(TerminalWasm::new(cols, rows)),
            app: D::build(seed),
            seed,
            idle: Duration::ZERO,
            last: Instant::now(),
            frame: 0,
        }
    }

    /// Advances the match by the real time elapsed since the previous call and
    /// returns the ANSI bytes to write to the terminal emulator (empty if
    /// nothing changed).
    pub fn tick(&mut self) -> String {
        let now = Instant::now();
        let delta = now.duration_since(self.last);
        self.last = now;
        let frame = retroglyph_core::Frame {
            delta,
            frame: self.frame,
        };
        self.frame = self.frame.wrapping_add(1);
        let _ = self.app.update(&mut self.term, &frame);

        if D::is_over(&self.app) {
            self.idle = self.idle.saturating_add(delta);
            if self.idle >= RESTART_AFTER {
                self.seed = self.seed.wrapping_add(1);
                self.app = D::build(self.seed);
                self.idle = Duration::ZERO;
            }
        } else {
            self.idle = Duration::ZERO;
        }

        self.term.backend_mut().take_output()
    }

    /// Forwards a key from the page (see `decode_key_event`'s encoding) into
    /// the running App.
    pub fn key(&mut self, code: u32, mods: u8) {
        if let Some(event) = retroglyph_terminal_wasm::decode_key_event(code, mods) {
            self.term
                .backend_mut()
                .push_event(retroglyph_core::event::Event::Key(event));
        }
    }

    /// Forwards a mouse event from the page (see `decode_mouse_event`'s
    /// encoding) into the running App.
    ///
    /// `x`/`y` are cell coordinates, not pixels: the page divides by the
    /// terminal emulator's cell size before calling this, since the App only
    /// ever thinks in cells.
    pub fn mouse(&mut self, x: u16, y: u16, action: u8, button: u8, mods: u8) {
        if let Some(event) =
            retroglyph_terminal_wasm::decode_mouse_event(x, y, action, button, mods)
        {
            self.term
                .backend_mut()
                .push_event(retroglyph_core::event::Event::Mouse(event));
        }
    }

    /// Resizes the grid to match the browser terminal.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(cols, rows);
    }
}

/// Emits the `wasm-bindgen` FFI for a concrete [`Demo`] type.
///
/// Generates the browser entry points `wasm_demo_init(cols, rows, seed)`,
/// `wasm_demo_tick() -> String`, `wasm_demo_key(code, mods)`,
/// `wasm_demo_mouse(x, y, action, button, mods)`, and
/// `wasm_demo_resize(cols, rows)`, backed by one thread-local [`WasmDemo`].
///
/// Call once, at the top level of a game's `examples/` entry, right after
/// defining the `Demo` impl:
///
/// ```ignore
/// struct CoupDemo;
/// impl turnbase_demos::Demo for CoupDemo { /* ... */ }
/// turnbase_demos::demo_entry!(CoupDemo);
/// ```
///
/// Expands to just a stub `fn main` off `wasm32` (so the example still builds
/// host-side for the workspace lint/test jobs); the `#[wasm_bindgen]` surface
/// is `wasm32`-only, since that is the only target that can export it.
#[macro_export]
macro_rules! demo_entry {
    ($demo:ty) => {
        // The real entry points are the wasm-bindgen exports below; `main` is
        // just the stub every example binary target needs. It constructs the
        // marker so it is not dead code on non-wasm hosts (the FFI that uses
        // it is wasm32-only); this never runs meaningfully.
        fn main() {
            let _ = <$demo as ::core::default::Default>::default();
        }

        #[cfg(target_arch = "wasm32")]
        const _: () = {
            ::std::thread_local! {
                static DEMO: ::std::cell::RefCell<
                    ::std::option::Option<$crate::WasmDemo<$demo>>,
                > = ::std::cell::RefCell::new(::std::option::Option::None);
            }

            /// Builds the demo at `cols` x `rows`, seeded from `seed`. Call
            /// once before the first tick.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_demo_init(cols: u16, rows: u16, seed: u32) {
                ::console_error_panic_hook::set_once();
                DEMO.with(|cell| {
                    *cell.borrow_mut() =
                        ::std::option::Option::Some($crate::WasmDemo::<$demo>::new(
                            cols,
                            rows,
                            ::core::convert::From::from(seed),
                        ));
                });
            }

            /// Advances one frame and returns the ANSI to write to the
            /// terminal emulator. Empty before `wasm_demo_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_demo_tick() -> ::std::string::String {
                DEMO.with(|cell| match cell.borrow_mut().as_mut() {
                    ::std::option::Option::Some(demo) => demo.tick(),
                    ::std::option::Option::None => ::std::string::String::new(),
                })
            }

            /// Queues a key event. No-op before `wasm_demo_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_demo_key(code: u32, mods: u8) {
                DEMO.with(|cell| {
                    if let ::std::option::Option::Some(demo) = cell.borrow_mut().as_mut() {
                        demo.key(code, mods);
                    }
                });
            }

            /// Queues a mouse event at cell `x`, `y`. No-op before
            /// `wasm_demo_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_demo_mouse(x: u16, y: u16, action: u8, button: u8, mods: u8) {
                DEMO.with(|cell| {
                    if let ::std::option::Option::Some(demo) = cell.borrow_mut().as_mut() {
                        demo.mouse(x, y, action, button, mods);
                    }
                });
            }

            /// Reports a new grid size (in cells). No-op before
            /// `wasm_demo_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_demo_resize(cols: u16, rows: u16) {
                DEMO.with(|cell| {
                    if let ::std::option::Option::Some(demo) = cell.borrow_mut().as_mut() {
                        demo.resize(cols, rows);
                    }
                });
            }
        };
    };
}
