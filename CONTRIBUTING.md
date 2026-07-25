# Contributing to turnbase

## Development

Prerequisites:

- Rust (latest stable; the MSRV is 1.88, enforced by a CI matrix job)
- Node.js (version in `.nvmrc`, for the Markdown/YAML formatters)
- [`just`](https://github.com/casey/just)

Optional, installed on demand by the recipes that need them: `cargo-nextest`, `cargo-llvm-cov`,
`cargo-deny`, `jq`.

### Workflow

`just check` is the gate before every commit. All clippy lints, including `pedantic` and `nursery`,
are denied, so a lint is a build failure rather than a warning.

| Command          | What it does                                                    |
| ---------------- | --------------------------------------------------------------- |
| `just check`     | Full gate: fmt-check, lint, test-all, doc                       |
| `just fix`       | Auto-fix formatting and the clippy lints that are auto-fixable  |
| `just fmt`       | Format Rust + Markdown/YAML/JSON                                |
| `just fmt-check` | Verify formatting without modifying (rustfmt + prettier)        |
| `just rustfmt`   | rustfmt check only                                              |
| `just prettier`  | prettier check only                                             |
| `just markdown`  | markdownlint                                                    |
| `just clippy`    | Clippy with `-D warnings` on all targets, all features          |
| `just lint`      | Clippy + markdownlint                                           |
| `just compile`   | `cargo check`, the fast iteration loop                          |
| `just test`      | nextest, no doctests                                            |
| `just test-doc`  | Doctests only                                                   |
| `just test-all`  | nextest + doctests, all features                                |
| `just test-ci`   | As `test-all`, under the `ci` nextest profile (emits JUnit XML) |
| `just deny`      | cargo-deny: advisories, licenses, bans, sources                 |
| `just hack`      | Build each crate with every feature enabled on its own          |
| `just typos`     | Spell-check source and prose                                    |
| `just machete`   | Detect unused dependencies                                      |
| `just taplo`     | Format TOML in place (`taplo-check` to verify)                  |
| `just actions`   | Lint the workflows (actionlint + zizmor)                        |
| `just doc`       | rustdoc including private items                                 |
| `just docs-site` | The full GitHub Pages site into `target/doc`                    |
| `just coverage`  | Coverage report, opened in a browser                            |

To preview the docs site locally, run `just docs-site` and serve `target/doc` over HTTP (not
`file://` — the WASM demos `fetch()` their module, which browsers block from a file origin).

## Commit messages and PR titles

This repo is squash-merge only, and PR titles follow
[Conventional Commits](https://www.conventionalcommits.org): `feat(core): add Pile::shuffle`,
`fix(bots): ...`, `docs(session): ...`. The squash-merge turns the PR title into the single commit
on `main`, which is what drives per-crate version bumps and changelogs. Commits inside your branch
are unconstrained.

**Scope** is the crate directory under `crates/*` your change touches: `core`, `bots`, `match`,
`simulator`, `protocol`, `session`, `cli`, `demos`, or `examples`. For changes that do not belong to
one crate, use `workspace` (tooling, CI, docs, release config) or `deps` (dependency bumps). A
scopeless title is accepted but `workspace` is preferred.

CI enforces this on the PR title (`.github/workflows/pr-title.yml`). It matters beyond tidiness: a
commit that does not conform is one the changelog generator will silently drop rather than flag.

## Labels

Mostly automatic. `c:<area>` labels are applied from the changed file paths and kept in sync as the
PR evolves, with the PR title's scope as a fallback for what paths cannot infer. New issues from
non-maintainers get `needs-triage`.

`.github/labels.yml` is the source of truth. To apply a change to it:

```sh
.github/scripts/sync-labels.sh              # create/update
.github/scripts/sync-labels.sh --delete-extra   # also remove labels absent from the file
```

It needs `gh` and `yq`, and is run by hand rather than in CI so no workflow needs label-write scope
on every push. Deleting is opt-in: a plain run reports extras without removing them.

## Dependency updates

Dependabot covers all three ecosystems (Cargo, GitHub Actions, and the npm formatter toolchain under
`tools/`) weekly, grouped so routine bumps arrive as one reviewable batch rather than a wall of
individual PRs. Majors are ungrouped, since those are the ones needing real review. All of it lands
under the `deps` scope.

## Adding a reference game

The ten games in `examples/` are a pressure test as much as a demo — each one picks at a different
corner of the `Game` trait. Before adding an eleventh, be able to say which corner it exercises that
no existing game does.

See [`examples/AGENTS.md`](examples/AGENTS.md) for the tier conventions, the required feature
layout, and the checklist.

## Crate layout

```text
crates/
  core/        turnbase            Game trait, State, Prng, Pile, ActivePlayers, effects
  bots/        turnbase-bots       Random, Minimax, Mcts, Ismcts
  match/       turnbase-match      The turn loop: Simulator, PlayerAgent. No UI or I/O
  simulator/   turnbase-simulator  Interactive retroglyph terminal client
  protocol/    turnbase-protocol   Typed request/response wire types
  session/     turnbase-session    The Session port: in-memory and file-backed hosts
  cli/         turnbase-cli        Generic runner (run, run_tui)
  demos/       turnbase-demos      In-browser WASM harness (publish = false)
examples/                          Ten reference games (publish = false)
```

The crates layer strictly: `core` has no opinions, `bots` and `match` build on it, `simulator`
renders, and `protocol`/`session` sit above for hosted play. A lower layer must never depend on a
higher one.

Only the seven library crates under `crates/*` are published; `demos` and every example are
`publish = false`. `tools/publishable-crates.sh` is the single source of truth for that split and
drives the docs site, the crates index, and `llms.txt`.

## Versions and dependencies

Every version requirement lives in the root `Cargo.toml` under `[workspace.dependencies]`. Members
inherit with `<dep> = { workspace = true }` and add their own `features` / `optional` on top. Add a
new shared dependency there, not in the member manifest.

The `retroglyph-*` crates are one upstream workspace and only work as a set. Bump them together;
bumping one alone leaves two incompatible copies of `retroglyph-core` in the lockfile, which fails
with `mismatched types` between two types that have identical names.

The seven publishable crates are **versioned independently** and each carries a literal `version` in
its own `[package]`, so a change confined to one leaf crate bumps only that crate. The
`[workspace.package] version` field is inherited by the example games only. You should not normally
edit any of these by hand; release tooling owns them.

## Reporting bugs

A determinism bug is the highest-severity class here. If you hit one, include the seed and the exact
sequence of actions — with those two, the failure is reproducible by construction, which is the
whole point of the design.
