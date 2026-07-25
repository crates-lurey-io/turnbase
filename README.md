# turnbase

[![CI](https://github.com/crates-lurey-io/turnbase/actions/workflows/test.yml/badge.svg)](https://github.com/crates-lurey-io/turnbase/actions/workflows/test.yml)
[![Docs](https://github.com/crates-lurey-io/turnbase/actions/workflows/docs.yml/badge.svg)](https://crates-lurey-io.github.io/turnbase/)
[![codecov](https://codecov.io/gh/crates-lurey-io/turnbase/graph/badge.svg)](https://codecov.io/gh/crates-lurey-io/turnbase)

Headless, deterministic turn-based game engine core for Rust.

`turnbase` defines any turn-based game once, as pure functions from state and
action to new state, and gets simulation, AI (minimax, MCTS, ISMCTS), and
headless playtesting for free. `crates/core` has no networking, no rendering,
and no async runtime in its dependency tree — everything is synchronous. See
[`ARCHITECTURE.md`](./ARCHITECTURE.md) for the design and the reasoning behind
it.

## Crates

| Crate | Description |
| --- | --- |
| [`crates/core`](./crates/core) | The `Game` trait and supporting types (`State`, `Prng`, `Pile`, effects). Published as `turnbase`. |
| [`crates/bots`](./crates/bots) | Search and policy bots: `RandomBot`, `Minimax`, `Mcts`, `Ismcts`. Published as `turnbase-bots`. |
| [`crates/match`](./crates/match) | The turn loop: `Simulator` and `PlayerAgent`, with no UI or I/O. Published as `turnbase-match`. |
| [`crates/simulator`](./crates/simulator) | Interactive retroglyph terminal client over a `turnbase-match` loop. Published as `turnbase-simulator`. |
| [`crates/protocol`](./crates/protocol) | Typed request/response wire types. Published as `turnbase-protocol`. |
| [`crates/session`](./crates/session) | The `Session` port: in-memory and file-backed hosts. Published as `turnbase-session`. |
| [`crates/cli`](./crates/cli) | Generic command-line runner (`run`, `run_tui`). Published as `turnbase-cli`. |
| [`examples/`](./examples) | Ten standalone reference games, each its own crate (see the table below). Not published. |
| [`crates/demos`](./crates/demos) | WASM harness that runs each reference game in the browser via `retroglyph-terminal-wasm`. Not published. |

The layer above the core (sessions, hosts, headless/interactive/networked
clients) is described in
[`docs/design/sessions-and-transports.md`](./docs/design/sessions-and-transports.md).

Only the seven `crates/*` libraries are published to crates.io and shipped in
the [API docs](https://crates-lurey-io.github.io/turnbase/crates/); the example
games and the demo harness (`publish = false`) are not.

## Reference games

Ten games, each its own crate with a one-line `main` over `turnbase-cli`.
They double as a pressure test: every row picks at a different corner of the
[`Game`](./crates/core/src/game.rs) trait.

| Game | Tier | What it exercises |
| --- | --- | --- |
| [`tic_tac_toe`](./examples/tic_tac_toe) | 0 | The whole trait at its smallest: perfect information, no chance, no triggers. |
| [`high_card`](./examples/high_card) | 0 | Committed chance (`PlayerId::CHANCE`) and hidden information at minimum size. |
| [`rock_paper_scissors`](./examples/rock_paper_scissors) | 0 | Simultaneous, secret moves (both seats active at once). |
| [`coup`](./examples/coup) | 1 | Bluffing over hidden roles with response windows; implements `Determinize`, so ISMCTS plays it. |
| [`minion_battle`](./examples/minion_battle) | 1 | Moves that cascade through triggered effects (deathrattles enqueue more). |
| [`risk`](./examples/risk) | 1 | Spatial state on a map graph and a long multi-phase turn. |
| [`blackjack`](./examples/blackjack) | 1 | A best-of-N match against a scripted dealer, with a bespoke custom TUI (not the shared dashboard). |
| [`hanabi`](./examples/hanabi) | 1 | Cooperative play whose visibility rule is the exact inverse of the default (you see everyone's hand but your own). |
| [`woodland`](./examples/woodland) | 1 | Two asymmetric factions (the enum-of-enums action convention) contesting a clearing ring. |
| [`spire_run`](./examples/spire_run) | 1 | A solo deckbuilding run: phase composition with a nested combat mini-game. |

Tier 0 is a `Game` impl plus serde derives, with a text `play`. Tier 1 adds a
[`PrintableGame`](./crates/simulator/src/ui.rs) impl to upgrade `play` to the
retroglyph dashboard (on by default; build `--no-default-features --features
cli` for text-only). Blackjack is the exception that proves a game can ship its
own terminal UI: it drives the `Simulator` through a hand-written `App` instead
of the shared dashboard.

## Quick start

Implement [`Game`](./crates/core/src/game.rs) for your game's rules, with a
`State` for one position and an `Action` for one decision:

```rust
use turnbase::{ActivePlayers, Game, PlayerId};

impl Game for TicTacToe {
    type State = Board;
    type Action = Move;
    type View = Board;

    fn new_initial_state(&self, seed: u64) -> Self::State { /* ... */ }
    fn num_players(&self) -> usize { 2 }
    fn active_players(&self, state: &Self::State) -> ActivePlayers { /* ... */ }
    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> { /* ... */ }
    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) { /* ... */ }
    // ...
}
```

Then drive it with any bot from `turnbase-bots`, or step through it by hand.
See [`examples/tic_tac_toe/src/lib.rs`](./examples/tic_tac_toe/src/lib.rs) for a
complete reference implementation.

## Playing the reference games

Each reference game is a standalone crate with a one-line `main` over
`turnbase-cli`, so it gets headless play, bot self-play, and interactive play
with no extra code:

```bash
# Watch two bots play tic-tac-toe:
cargo run -p tic_tac_toe -- self-play

# Play Coup yourself against bots, in a terminal dashboard:
cargo run -p coup -- play

# Drive a game headlessly, one action per process (agent- or script-friendly):
cargo run -p tic_tac_toe -- new --session game.json
cargo run -p tic_tac_toe -- act --session game.json --player 0 --action 4
cargo run -p tic_tac_toe -- query --session game.json --player 1
```

Every game takes the same subcommands (`new`, `query`, `act`, `self-play`,
`play`). Some games do not converge under uniform-random `self-play` (Risk, for
one), which the runner reports honestly rather than looping forever.

A Tier 1 `play` opens the interactive dashboard, which is a real session
controller, not just an auto-runner. Press `c` for the setup modal to make any
seat Human or pick its AI type (Random, MCTS, or ISMCTS where the game supports
it); `m` toggles Auto/Step, `Space` single-steps, `+`/`-` change speed, `p`
pauses, and `r` restarts with a fresh seed. A human seat plays through the
on-screen action menu, and the status bar lists the controls.

The dashboard takes the mouse as well as the keyboard: the wheel scrolls the
log strip (as do `PgUp`/`PgDn` and `Home`/`End`), its scrollbar is
click-to-jump and drag-to-scroll, and clicking a row in the action menu selects
it, with a second click on the selected row playing it.

## Live demos and docs

Every push to `main` publishes to <https://crates-lurey-io.github.io/turnbase/>:

- [Interactive demos](https://crates-lurey-io.github.io/turnbase/demos/) of
  each dashboard game, running the exact same `App` as the native client but
  compiled to WebAssembly and rendered into `xterm.js`. They self-play out of
  the box; the same setup modal and controls work in the browser. The grid is
  fitted to the window (the font scales down until at least 80 columns fit), the
  mouse and touch gestures are forwarded as terminal mouse events, and an
  on-screen key bar drives a session on a device with no keyboard.
- [API docs](https://crates-lurey-io.github.io/turnbase/crates/) for the seven
  published libraries, with per-crate `llms.txt` for coding assistants.
- A [coverage report](https://crates-lurey-io.github.io/turnbase/coverage/).

Build the whole site locally with `just docs-site`.

## Contributing

This uses [`just`][] to run the same checks as CI:

- `just check` — format and lint (fmt + clippy, pedantic/nursery denied).
- `just test-all` — unit tests and doctests, all features.
- `just doc` — build docs with warnings denied.
- `just coverage` — generate and open a coverage report.

See the [`Justfile`](./Justfile) for the full list.

[`just`]: https://crates.io/crates/just

## License

Dual-licensed under [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE), at
your option.
