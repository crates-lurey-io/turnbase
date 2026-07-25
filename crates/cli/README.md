# turnbase-cli

[![crates.io](https://img.shields.io/crates/v/turnbase-cli.svg)](https://crates.io/crates/turnbase-cli)
[![docs.rs](https://docs.rs/turnbase-cli/badge.svg)](https://docs.rs/turnbase-cli)

Generic command-line runner for Turnbase games.

Turns a [`turnbase`] game into a playable binary with a one-line `main`: headless self-play, text
play, or the full terminal dashboard.

```rust,ignore
fn main() {
    turnbase_cli::run_tui(MyGame::default());
}
```

The `tui` feature (on by default) pulls in the dashboard; disable it for a text-only build that
never links a terminal UI.

## Install

```toml
[dependencies]
turnbase-cli = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
