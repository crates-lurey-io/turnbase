# Style guide

Rust API and code style conventions for this workspace. `ARCHITECTURE.md` covers _why_ the design is
shaped the way it is; this covers _how_ to write code within it. `AGENTS.md` has the short list of
correctness invariants that must never be broken.

## Lints

The workspace sets, in the root `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
nursery = { level = "deny", priority = -1 }
```

Every crate opts in with `[lints] workspace = true`. A new crate that forgets this line is linted
far more weakly than the rest of the workspace, so add it.

`deny`, not `warn`: a lint is a build failure. When a lint is genuinely wrong for a specific site,
`#[allow(...)]` it at the narrowest possible scope with a comment saying why. `crates/core`'s
`chance_outcomes` is the model:

```rust
// Outcome counts are small; the reciprocal is exact enough as a weight.
#[allow(clippy::cast_precision_loss)]
let probability = 1.0 / actions.len() as f64;
```

For proper nouns that `clippy::doc_markdown` flags as un-backticked code (`OpenSpiel`, `PettingZoo`,
`ISMCTS`, `GGPO`), extend `doc-valid-idents` in `clippy.toml` rather than adding an `#[allow]` at
each site. Keep the trailing `".."` entry, which preserves clippy's built-in list.

## Errors and panics

**Fallible public entry points return `Result`.** The engine's `Error` is a plain enum,
`Clone + PartialEq + Eq + Debug`, implementing `Display` and `std::error::Error` by hand. No
`thiserror`, no `anyhow` in library code.

**`Error` is `#[non_exhaustive]`.** Adding a variant must not be a breaking change for downstream
matchers. Its doc comment says so explicitly ("More variants may be added, so match with a wildcard
arm"), which is the convention to follow for any new public enum that is expected to grow.

**Use `.expect("invariant")`, not bare `.unwrap()`, in non-test code.** The message states the
invariant that makes it safe, phrased as an assertion of fact:

```rust
best.expect("a fully expanded node has children")
.expect("non-terminal state has a legal action")
.expect("seat index fits in u32")
```

Bare `.unwrap()` is fine in `#[cfg(test)]` code and doctests, where a panic _is_ the failure report.
This is close to universal in the current tree: every bare `unwrap()` in `crates/*/src` is inside a
test module or a doc example, except two in `crates/bots/src/ismcts.rs` that should be given
messages when next touched.

**Panicking is acceptable only for genuine programmer error**, and `apply` is the notable case: it
documents that it assumes a legal action, because checking belongs in `apply_cloned` and search
calls `apply` in its hot loop. Do not add validation there.

## Naming and API shape

- **Per-match configuration lives on the implementing value (`&self`), not in `State`.** Player
  count, board size, and variant flags belong on the game struct so `State` stays lean and cheap to
  clone, since cloning is the default backtracking primitive.
- **Prefer returning a materialized `Vec` over an iterator where callers pass over the result more
  than once.** `chance_outcomes` documents this reasoning: callers sum the weights and then sample.
  Do not reach for an iterator by reflex when the collection is small and traversed twice.
- **Default trait methods are for the common case; overriding is the documented escape hatch.**
  `is_legal` defaults to membership in `legal_actions`, overridable for decision points too large to
  enumerate. `chance_outcomes` defaults to uniform. `step_reward` defaults to zero. When adding a
  trait method, ask whether a default plus a documented override is better than making every
  implementor write it.
- **Opt-in capability traits over fatter core traits.** `Reversible` and `Determinize` are separate
  traits a game implements to unlock a specific capability (make/unmake search, ISMCTS), not methods
  on `Game`. Follow this shape for future capabilities rather than growing `Game`.
- **Faction asymmetry uses the enum-of-enums `Action` convention** (see `examples/woodland`), not a
  trait object or a union of every faction's fields.

## Documentation comments

- **Every public item has a doc comment.** Rustdoc runs with `RUSTDOCFLAGS=-Dwarnings` in CI, so a
  broken intra-doc link fails the build.
- **Document the contract, not the implementation**: what it returns, what it assumes, what it
  errors on, and any invariant the caller must uphold. `Game::apply`'s "Assumes the action is legal"
  and `Reversible`'s RNG-snapshot paragraph are the model.
- **`# Errors` sections on anything returning `Result`**, naming the variants and their conditions.
- **Link related items with intra-doc links** (`[`Self::active_players`]`, `[`Prng::position`]`) so
  rustdoc cross-references stay live and CI catches rot.
- **Explain the non-obvious "why" inline.** This codebase deliberately carries long comments where a
  decision has a real rationale or a past incident behind it. Do not strip them for brevity; do not
  add ones that merely restate the code.

## Formatting

rustfmt defaults, no `rustfmt.toml`, so `max_width = 100`. `wrap_comments` is off by default, which
means rustfmt never rewraps prose comments for you: **wrap doc comments near 100 columns, not 80**.

Markdown, YAML, and JSON are formatted by prettier (`printWidth: 100`, `proseWrap: always`) and
linted by markdownlint. Both run through `just fmt` / `just fmt-check`. TOML is not formatted
automatically yet.

## Determinism

The rules that keep replays reproducible are in `AGENTS.md` and `ARCHITECTURE.md`. The short version
for day-to-day code review: no `HashMap`/`HashSet` in anything that influences game logic, no
randomness outside the `Prng` in `State`, and ordered iteration everywhere.
