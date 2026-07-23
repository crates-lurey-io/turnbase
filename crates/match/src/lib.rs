//! Turn-loop orchestration for [`turnbase`]: seat agents and a match stepper.
//!
//! [`Simulator`] is plain, synchronous bookkeeping over a [`turnbase::Game`]:
//! it decides whose turn it is, asks a [`turnbase_bots::Bot`] for an action or
//! waits on a human, applies the result, and logs it. There is no terminal,
//! rendering, socket, or file I/O in its dependency tree, so it runs
//! identically under `cargo test`, behind a `turnbase-simulator` dashboard, or
//! inside a headless CLI.
//!
//! This is the shared substrate the interactive and headless clients both run
//! on. Rendering lives one layer up in `turnbase-simulator`; persistence and
//! the request/response port live in `turnbase-session`.

mod simulator;

pub use simulator::{PlayerAgent, Simulator};
