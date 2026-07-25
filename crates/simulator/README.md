# turnbase-simulator

[![crates.io](https://img.shields.io/crates/v/turnbase-simulator.svg)](https://crates.io/crates/turnbase-simulator)
[![docs.rs](https://docs.rs/turnbase-simulator/badge.svg)](https://docs.rs/turnbase-simulator)

Interactive terminal client for the Turnbase engine.

A [retroglyph](https://github.com/crates-lurey-io/retroglyph) terminal dashboard over a
[`turnbase-match`] turn loop: step through a match, watch bots play, inspect per-seat views.

A game opts in by implementing `PrintableGame`. The runner is backend-generic, so the same dashboard
drives a native terminal or a browser canvas; disable the default `crossterm` feature for a wasm
build.

## Install

```toml
[dependencies]
turnbase-simulator = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
