# turnbase-bots

[![crates.io](https://img.shields.io/crates/v/turnbase-bots.svg)](https://crates.io/crates/turnbase-bots)
[![docs.rs](https://docs.rs/turnbase-bots/badge.svg)](https://docs.rs/turnbase-bots)

Search and policy bots for the Turnbase engine.

Ready-made opponents for any [`turnbase`] game:

| Bot       | Use it for                                                                   |
| --------- | ---------------------------------------------------------------------------- |
| `Random`  | A baseline, and for shaking out illegal-move bugs                            |
| `Minimax` | Small perfect-information games, with alpha-beta pruning                     |
| `Mcts`    | Larger perfect-information games, or where no evaluation function is obvious |
| `Ismcts`  | Hidden-information games, over a game's `Determinize` impl                   |

## Install

```toml
[dependencies]
turnbase-bots = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
