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
| [`examples`](./examples) | Reference games implemented against the engine: tic-tac-toe, rock-paper-scissors, high-card, Coup, minion battle, Risk. |

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
See [`examples/src/tic_tac_toe.rs`](./examples/src/tic_tac_toe.rs) for a
complete reference implementation.

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
