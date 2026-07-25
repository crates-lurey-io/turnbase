# turnbase-session

[![crates.io](https://img.shields.io/crates/v/turnbase-session.svg)](https://crates.io/crates/turnbase-session)
[![docs.rs](https://docs.rs/turnbase-session/badge.svg)](https://docs.rs/turnbase-session)

The Session port for the Turnbase engine.

One request/response interface for hosting a match, with two implementations: an in-memory host for
tests and single-process play, and a file-backed host that persists between calls.

Because a [`turnbase`] state serializes with its random generator position included, a file-backed
session resumes from a single snapshot rather than replaying the action log.

## Install

```toml
[dependencies]
turnbase-session = "0.1"
```

## Where this sits

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace. See the
[workspace README](https://github.com/crates-lurey-io/turnbase#crates) for the full crate list and
[ARCHITECTURE.md](https://github.com/crates-lurey-io/turnbase/blob/main/ARCHITECTURE.md) for the
design.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
