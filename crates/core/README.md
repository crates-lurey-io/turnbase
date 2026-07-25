# turnbase

[![crates.io](https://img.shields.io/crates/v/turnbase.svg)](https://crates.io/crates/turnbase)
[![docs.rs](https://docs.rs/turnbase/badge.svg)](https://docs.rs/turnbase)

Headless, deterministic turn-based game engine core.

Define a turn-based game once, as pure functions from state and action to new state, and get
simulation, AI, and headless playtesting for free.

No networking, no rendering, and no async runtime in the dependency tree. Everything is synchronous,
and randomness lives inside the state as a counter-based generator, so a match snapshots and resumes
in O(1) and replays identically from the same seed.

The trait to implement is [`Game`]. Optional capability traits unlock specific engines:
[`Reversible`] for make/unmake search, [`Determinize`] for information-set MCTS.

## Install

```toml
[dependencies]
turnbase = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
