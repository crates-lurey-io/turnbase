# turnbase-match

[![crates.io](https://img.shields.io/crates/v/turnbase-match.svg)](https://crates.io/crates/turnbase-match)
[![docs.rs](https://docs.rs/turnbase-match/badge.svg)](https://docs.rs/turnbase-match)

Turn-loop orchestration for the Turnbase engine.

The turn loop, with no UI and no I/O: `Simulator` drives a [`turnbase`] game forward, asking each
seat's `PlayerAgent` for a decision and resolving chance nodes in between.

This is the layer that turns a `Game` impl into a playable match. A terminal client, a web demo, and
a headless self-play harness all sit on top of the same loop.

## Install

```toml
[dependencies]
turnbase-match = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
