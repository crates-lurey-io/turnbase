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
| [`examples/`](./examples) | Standalone reference games, each its own crate: tic-tac-toe, coup, high-card, rock-paper-scissors, minion-battle, risk. |

The layer above the core (sessions, hosts, headless/interactive/networked
clients) is described in
[`docs/design/sessions-and-transports.md`](./docs/design/sessions-and-transports.md).

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

# Play Coup yourself (seat 0) against three bots, in a terminal dashboard:
cargo run -p coup -- play

# Drive a game headlessly, one action per process (agent- or script-friendly):
cargo run -p tic_tac_toe -- new --session game.json
cargo run -p tic_tac_toe -- act --session game.json --player 0 --action 4
cargo run -p tic_tac_toe -- query --session game.json --player 1
```

Tic-tac-toe is "Tier 0": a `Game` impl plus serde derives, no rendering code,
and its binary links no terminal UI. Coup is "Tier 1": it adds a `PrintableGame`
impl to upgrade `play` to the retroglyph dashboard. Everything else is identical
between them.

All six reference games run the same way (`cargo run -p <game> -- <command>`):
`tic_tac_toe`, `coup`, `high_card`, `rock_paper_scissors`, `minion_battle`, and
`risk`. Only `coup` is Tier 1; the rest are Tier 0. Some games do not converge
under uniform-random `self-play` (Risk, for one), which the runner reports
honestly rather than looping forever.

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
