# AGENTS.md — reference games

The ten crates here are a pressure test as much as a demo. Each one picks at a different corner of
the [`Game`](../crates/core/src/game.rs) trait, and the set exists to keep the core honest: if a
game shape cannot be expressed cleanly, that is a finding about the trait, not about the game.

Every crate here is `publish = false`.

## Before adding an eleventh game

Be able to name the corner it exercises that no existing game does. "Another card game" is not a
reason; "the first game whose legal action set is too large to enumerate" is. The current coverage:

| Game                  | Tier | Corner it exercises                                                     |
| --------------------- | ---- | ----------------------------------------------------------------------- |
| `tic_tac_toe`         | 0    | The whole trait at its smallest: perfect info, no chance, no triggers   |
| `high_card`           | 0    | Committed chance (`PlayerId::CHANCE`) and hidden info, minimum size     |
| `rock_paper_scissors` | 0    | Simultaneous secret moves (both seats active at once)                   |
| `coup`                | 1    | Bluffing with response windows; implements `Determinize`, so ISMCTS     |
| `minion_battle`       | 1    | Moves cascading through triggered effects                               |
| `risk`                | 1    | Spatial state on a map graph, long multi-phase turn                     |
| `blackjack`           | 1    | Best-of-N match vs a scripted dealer; ships its own bespoke TUI         |
| `hanabi`              | 1    | Visibility rule inverted from the default (you see all hands but yours) |
| `woodland`            | 1    | Two asymmetric factions, the enum-of-enums `Action` convention          |
| `spire_run`           | 1    | Solo deckbuilding run: phase composition with a nested combat mini-game |

## Tiers

**Tier 0** is a `Game` impl plus serde derives, with a text runner. Three games.

```toml
[features]
default = ["cli"]
cli = ["dep:turnbase-cli"]
```

**Tier 1** adds a [`PrintableGame`](../crates/simulator/src/ui.rs) impl, which upgrades the runner
to the shared retroglyph dashboard. On by default, so `cargo run -p <game>` opens the dashboard.

```toml
[features]
default = ["ui"]
cli = ["dep:turnbase-cli"]
ui = ["cli", "dep:turnbase-simulator", "dep:retroglyph-core", "turnbase-cli/tui"]
```

## The wasm-safe feature subset

Five games also ship as in-browser WASM demos through `crates/demos`. Those demos render with a
browser terminal backend, so **the dashboard stack has to build for `wasm32`, which crossterm does
not**. Those games therefore factor the backend-generic part of `ui` into its own feature that
`crates/demos` depends on directly:

```toml
printable = ["dep:turnbase-simulator", "dep:retroglyph-core"]
ui = ["cli", "printable", "turnbase-cli/tui"]
```

`coup`, `risk`, `minion_battle`, and `woodland` use exactly this. `blackjack` is the same idea under
a different name: it has its own TUI rather than a `PrintableGame` impl, so its wasm-safe feature is
`app` (the backend-generic `App`), and `ui = ["cli", "app", "dep:retroglyph-crossterm"]` adds the
native backend on top.

`hanabi` and `spire_run` deliberately have **no** `printable` feature and inline those deps into
`ui`, because they do not ship as web demos and so need no wasm-safe subset. This is the rule, not
drift:

> A game has a wasm-safe feature subset if and only if it is a web demo.

If you add a game to `crates/demos`, you must factor out `printable` at the same time. If you add a
game that is not a demo, do not add `printable` speculatively.

## Checklist for a new game

1. `Cargo.toml`: `publish = false`, `[lints] workspace = true`, `version/edition/license/repository`
   inherited from the workspace, and the tier's feature block above.
2. `src/lib.rs` with the `Game` impl; `src/main.rs` as a one-line `main` over `turnbase-cli`. Tier 1
   adds `src/ui.rs` with the `PrintableGame` impl.
3. `[[bin]]` with `required-features = ["cli"]`, so a `--no-default-features` build does not try to
   link a binary with no runner.
4. Add the crate to the root `Cargo.toml` `members` list.
5. Add a row to `README.md`'s reference-game table naming the corner it exercises, and to the table
   above.
6. Implement `Determinize` if the game has hidden information and you want ISMCTS to play it. Read
   its invariant in `AGENTS.md` first: preserve everything the observer can already see, resample
   only what they cannot, and draw from the passed `rng`.
7. Tests. Unit tests next to the code at minimum; `coup`, `minion_battle`, `risk`, and `woodland`
   also have integration tests under `tests/` and are the model for a game with real rules
   interactions.
8. `just check`.

## Rules specific to this directory

- **Games must not reach for `HashMap`/`HashSet`** in anything affecting play. Same determinism
  reason as the engine; see the root `AGENTS.md`.
- **Randomness comes from the `Prng` inside `State`.** A game that samples from anywhere else breaks
  replay and snapshot/resume for everyone.
- **A game crate never depends on another game crate.** Shared helpers belong in `crates/core` (if
  general) or are duplicated (if not). The one exception is `crates/bots`' dev-dependencies, which
  pull in `tic_tac_toe`, `high_card`, and `coup` as test fixtures.
- **Lower layers stay lower.** A game depends on `turnbase` and optionally `turnbase-cli` /
  `turnbase-simulator`. Nothing here should be depended on by `crates/*` outside dev-dependencies.
