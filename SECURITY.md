# Security Policy

## Supported versions

Pre-1.0. Only the latest published version of each crate receives fixes; there are no maintained
release branches. See [`RELEASING.md`](RELEASING.md) for the versioning policy.

## Reporting a vulnerability

Report privately through
[GitHub Security Advisories](https://github.com/crates-lurey-io/turnbase/security/advisories/new),
not a public issue.

Include the affected crate and version, what an attacker can do, and ideally a seed plus action
sequence that reproduces it. Because the engine is deterministic, those two things make any
behavioral finding reproducible by construction.

Expect an initial response within a week.

## Scope

This is a game-logic library. It has no network stack, no async runtime, and `unsafe_code` is
`forbid` workspace-wide, so the classic memory-safety and remote-attack categories mostly do not
apply. What is in scope:

- **Deserialization of untrusted state.** `turnbase-session` and `turnbase-protocol` accept
  serialized game state. A panic, unbounded allocation, or infinite loop reachable from
  attacker-controlled input is a valid report.
- **Determinism breaks.** A way to make two replays of the same seed diverge is a correctness bug
  with security consequences for anyone using this to arbitrate multiplayer results.
- **Hidden-information leaks.** A way to observe another player's private state through the public
  API or a `view` projection, given the engine's public/private split.

Out of scope: denial of service from a caller's own deliberately expensive game rules, bot search
depth chosen by the caller, and anything requiring the attacker to already control the host process.
