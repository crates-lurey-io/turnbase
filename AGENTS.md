# AGENTS.md

`turnbase` is a headless, deterministic turn-based game engine. A game is defined once as pure
functions from state and action to new state, and gets simulation, AI (minimax, MCTS, ISMCTS), and
headless playtesting for free.

Workspace: a Cargo workspace with eight crates under `crates/*` and ten reference games under
`examples/*`. There is no single-crate `src/` root; consumers depend on `turnbase` (the core) plus
whichever layers they need. For the crate list see `README.md`; for the design and its rationale see
`ARCHITECTURE.md`.

## Correctness gate

**Run `just check` before every commit.** It runs fmt-check (rustfmt + prettier), lint (clippy +
markdownlint), the full test suite with all features, and rustdoc.

```sh
just check        # full gate -- must pass before committing
just fix          # auto-fix formatting + clippy
just compile      # fast type-check loop
just test         # nextest, no doctests
just test-all     # nextest + doctests, all features
just deny         # cargo-deny: advisories, licenses, bans, sources
just docs-site    # build the full Pages site locally
```

Clippy runs with `all`, `pedantic`, and `nursery` at **deny** (workspace `[lints]`), so a lint is a
build failure, not a warning. `unsafe_code` is `forbid`. Proper nouns that `clippy::doc_markdown`
would otherwise flag live in `clippy.toml`'s `doc-valid-idents`; add to that list rather than
sprinkling `#[allow]`.

For a quick loop: `just compile` for type errors, then `just check` before committing.

## The invariants that must not be broken

These are what make the engine's headline properties (O(1) snapshot/resume, exact undo, reproducible
replays) true. `ARCHITECTURE.md` has the full reasoning; this is the short list of things that
silently corrupt correctness if violated.

- **Never use `HashMap`/`HashSet` anywhere a value influences game logic.** Rust's std hasher
  randomizes iteration order per process, so any code that consumes RNG or resolves buffered actions
  while walking a hashed collection desyncs two replays of the same seed. `State`'s private map is a
  `BTreeMap` and `ActivePlayers` is ordered, deliberately. See "Determinism and RNG".
- **Randomness comes from the generator inside `State`, never from a side channel.** `apply` takes
  no `&mut dyn RngCore`. A `Prng` is a counter-based generator whose entire position is a small
  `Copy` value, which is exactly what makes serialization and undo work. Do not introduce a
  float-based or trait-object generator.
- **`Reversible::UndoRecord` must snapshot the generator position before the move.** A move may
  consume a variable number of draws, so the pre-move position cannot be recovered by counting. The
  clone path gets this for free; make/unmake does not. A wrong `undo` corrupts search silently
  rather than crashing.
- **`Determinize::determinize` must preserve everything the observer can already see.** Only
  resample what they cannot see, and draw that randomness from the passed `rng`, not from the state,
  so repeated calls explore different worlds. Violating this makes ISMCTS unsound in a way tests
  will not obviously catch.
- **Hidden information goes in `State`'s private map, not in `public`.** Redaction is mechanical
  precisely because there is no field to forget to strip. Hanabi is the documented inversion of the
  default rule; read that section before adding another.
- **`apply` assumes legality.** Checking belongs in `apply_cloned` and the other checked entry
  points. Do not add validation to `apply`; search calls it in its hot loop.

## Testing

`just test-all` runs everything. Tests are unit tests in `#[cfg(test)] mod tests` next to the code,
plus integration tests under `examples/*/tests/` for the four games that have them. `proptest` is
used where a property is the real specification (`crates/bots/src/minimax.rs`,
`examples/high_card`).

When adding engine behavior, the highest-value test is usually a determinism one: same seed twice,
assert identical outcomes, and for anything touching serde, assert that snapshot-then-resume
continues with identical rolls (`crates/core/src/serde_roundtrip.rs` is the model).

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org), scoped to the crate a change touches:
`feat(core): ...`, `fix(bots): ...`, `docs(session): ...`.

Valid scopes: `core`, `bots`, `match`, `simulator`, `protocol`, `session`, `cli`, `demos`,
`examples`. For changes that do not belong to one crate use `workspace` (tooling, CI, root docs,
release config) or `deps` (dependency bumps). A scopeless title is accepted, but prefer `workspace`
over omitting the scope.

The convention applies to **PR titles**, not individual commits. The repo is squash-merge only with
the squash message defaulting to the PR title, so the PR title becomes the single commit on `main`.
Work-in-progress commits inside a branch are unconstrained.

This is load-bearing rather than cosmetic: the release automation reads the squashed history to
compute per-crate version bumps and changelogs, and a changelog generator configured to filter
unconventional commits will silently drop a non-conforming one rather than fail.

Enforced by `.github/workflows/pr-title.yml`, which validates the title against the grammar and the
scope list above.

### Labels

`.github/labels.yml` is the source of truth, synced with `.github/scripts/sync-labels.sh`. `c:` area
labels are applied automatically: from changed paths by `.github/labeler.yml`, and as a fallback
from the PR title's scope. You rarely need to set one by hand.

| Label            | Meaning                                                                            |
| ---------------- | ---------------------------------------------------------------------------------- |
| `c:<area>`       | Which crate or area the change touches. Auto-applied and kept in sync.             |
| `breaking`       | Semver-breaking change.                                                            |
| `skip-changelog` | Keep this PR out of the generated changelog. Inert until release automation lands. |
| `needs-triage`   | New issue from a non-maintainer, not yet reviewed. Auto-applied.                   |
| `blocked`        | Waiting on an external dependency or decision.                                     |

**Never squash commits.** Prefer multiple well-described commits; use `jj new -m` to start the next
one. Split by path with `jj split [FILESETS]`, never interactively. Always pass `-m` to anything
that would otherwise open `$EDITOR`.

## Docs

| File                                     | What it covers                                                 |
| ---------------------------------------- | -------------------------------------------------------------- |
| `README.md`                              | Overview, crate table, the ten reference games and their tiers |
| `ARCHITECTURE.md`                        | The design and why: trait shape, determinism, hidden info, RNG |
| `STYLE_GUIDE.md`                         | Rust API and code style conventions                            |
| `CONTRIBUTING.md`                        | Setup, command reference, PR workflow                          |
| `docs/design/sessions-and-transports.md` | The layer above the core: sessions, hosts, transports          |
| `examples/AGENTS.md`                     | Checklist for adding or changing a reference game              |

Nested `AGENTS.md` files apply to their subtree; read them before working there.
