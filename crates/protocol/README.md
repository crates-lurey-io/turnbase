# turnbase-protocol

[![crates.io](https://img.shields.io/crates/v/turnbase-protocol.svg)](https://crates.io/crates/turnbase-protocol)
[![docs.rs](https://docs.rs/turnbase-protocol/badge.svg)](https://docs.rs/turnbase-protocol)

Transport-agnostic wire types for the Turnbase engine.

Typed request/response types for driving a match over a boundary, with no transport opinion of its
own: the same types serialize over HTTP, a WebSocket, a pipe, or a function call.

Paired with [`turnbase-session`], which implements the host side.

## Install

```toml
[dependencies]
turnbase-protocol = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
