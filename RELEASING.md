# Releasing

Releases are automated with [release-plz](https://release-plz.dev). Day to day you never bump a
version, edit a changelog, or push a tag by hand. You review PRs and, when you want to ship, merge a
single machine-generated Release PR.

## The short version

1. Land normal PRs on `main`. Each PR **title** is a Conventional Commit; the squash-merge turns it
   into `main`'s commit message.
2. release-plz keeps a standing **Release PR** open, continuously recomputing per-crate version
   bumps and changelog entries from that history.
3. When you want to release, **merge the Release PR**. That merge is the only approval.
4. CI publishes every crate whose version is not yet on crates.io, then tags it and creates the
   GitHub release.

If a push to `main` has nothing releasable, no Release PR appears and nothing happens.

## Versioning

**Per-crate, independent versions.** Each of the seven publishable crates (`turnbase`,
`turnbase-bots`, `turnbase-match`, `turnbase-simulator`, `turnbase-protocol`, `turnbase-session`,
`turnbase-cli`) carries and bumps its own version. `turnbase-demos` and every crate under
`examples/` are `publish = false` and never ship.

**Cascade is expected, not lockstep.** Every crate path-depends on `turnbase`, so a bump to the core
updates each dependent's version requirement, which bumps those dependents too. That looks like
lockstep for any change touching the core, but it isn't: a change isolated to a leaf crate
(`turnbase-cli`, `turnbase-session`, `turnbase-protocol`) bumps that crate alone. The
independent-versioning benefit is real only for those leaf-local changes.

### Pre-1.0 SemVer

While a crate's major version is `0`, Cargo shifts the breaking-change signal down one slot:

- **MINOR** (`0.1.0` -> `0.2.0`): breaking change. The pre-1.0 equivalent of a major bump.
- **PATCH** (`0.1.0` -> `0.1.1`): backwards-compatible fix or addition.
- **MAJOR** stays `0` until a deliberate decision to stabilize at `1.0.0`.

This is also why `.github/dependabot.yml` groups only `patch` updates: Dependabot classifies by the
version number alone, so it reports a breaking `0.2 -> 0.3` upgrade of a dependency as "minor".

### MSRV

Currently `1.88`, set in `[workspace.package].rust-version` and enforced by the `compile 1.88` CI
job. The floor comes from retroglyph. Treat an MSRV bump as at least a minor (pre-1.0 breaking)
bump.

## Conventional Commits: PR titles, not commits

Enforced on **PR titles only** (`.github/workflows/pr-title.yml`). The repo is squash-merge only, so
the title becomes the single commit on `main`, keeping the history release-plz reads fully
conventional while work-in-progress commits stay unconstrained.

`cliff.toml` sets `filter_unconventional = true`, so a non-conforming commit is **silently dropped**
from the changelog rather than flagged. That is the real reason the CI check exists.

## Declaring breaking changes

**Do not put `!` on a commit for an ordinary API-signature break.** `semver_check = true` in
`release-plz.toml` runs `cargo-semver-checks` while computing the Release PR and detects and bumps
correctly on its own. It is the authority, not the commit message.

**Why this matters in a monorepo with atomic, cross-crate commits:** release-plz attributes a
commit's Conventional Commit classification, `!` included, to **every crate whose packaged files
that commit touches** -- by file path, not by the stated `type(scope)`. One atomic commit that
breaks `crates/core` and also touches `crates/simulator` (a mechanical fix needed only because of
the core change) applies the `!` to both, even though the simulator's own API is untouched. Since
atomic cross-crate commits are the point of a monorepo, avoid this by not adding `!` where it is not
needed, not by splitting commits.

**Reserve `!` / a `BREAKING CHANGE:` footer for what `cargo-semver-checks` cannot see:** a
behavioral break with unchanged public signatures. Same types, same shapes, different runtime
meaning. That is rare. When it happens, keep the commit scoped to the crate actually breaking.

`cargo-semver-checks` runs at two points:

- **At release time**, inside release-plz. The authority.
- **At PR time** (`check-semver.yml`), non-blocking and informational, comparing against the PR's
  own base rather than crates.io. Comparing against the registry would make every PR stacked on an
  unreleased breaking change report breaking too. It syncs the `breaking` label and a report comment
  so the finding is visible without opening the Actions run, and leaves both untouched if the tool
  fails to reach a verdict, since an inconclusive run is not evidence either way.

## Keeping a PR out of the changelog

Either apply the `skip-changelog` label, or put `changelog: ignore` in the squash commit body.

## First-time setup

Both steps below are repository-owner actions that cannot be scripted from here, and the release job
fails without them.

1. **crates.io Trusted Publishing**, per crate. On crates.io go to each crate's Settings -> Trusted
   Publishing and add a GitHub publisher: owner `crates-lurey-io`, repository `turnbase`, workflow
   `release-plz.yml`, environment `release`.

   **Per crate is literal.** The token minted via OIDC is scoped to exactly the crates carrying a
   matching config, so a workspace with one crate configured publishes that crate and then fails
   with `403 Forbidden: The provided access token is not valid for crate <next>`. All seven need
   their own config.

   The workflow filename must match exactly, or crates.io rejects the exchange with
   `does not match the workflow filename ... in the JWT`. Both errors only surface at publish time.

   **Chicken-and-egg:** a Trusted Publishing config can only be created for a crate that already
   exists on crates.io (RFC 3691), so a brand-new crate has no settings page. Reserve the name first
   with a placeholder publish, then configure. All seven names are already reserved; the placeholder
   shape is a standalone crate outside this workspace with `version = "0.0.0-reserved"` and a
   one-line `src/lib.rs` reading `//! Name reservation placeholder for <name>. Not yet implemented.`

2. **The `release` GitHub environment**, with a branch policy restricting it to `main`. Already
   created.

3. **Approve the first Release PR's workflow runs.** GitHub gates workflow runs on PRs from
   first-time contributors, and `github-actions[bot]` counts as one until it has a merged PR. Until
   then every Release PR shows `action_required` and stays `BLOCKED` on the missing `required`
   check. Approve from the Actions tab, or:

   ```sh
   gh run list --branch <release-plz-branch> --json databaseId,conclusion \
     --jq '.[] | select(.conclusion=="action_required") | .databaseId' \
     | xargs -I{} gh api -X POST repos/crates-lurey-io/turnbase/actions/runs/{}/approve
   ```

   Approve runs for one commit at a time. CI's concurrency group has `cancel-in-progress: true`, so
   approving runs for two commits at once makes the newer cancel the older, and `required` treats a
   cancelled run as failure.

There is deliberately no `CARGO_REGISTRY_TOKEN` secret: Trusted Publishing exchanges the workflow's
OIDC token for a short-lived one at publish time.

## Known gotchas

- **`release-plz-pr` must wait for `release-plz-release`.** Both trigger on the same push to `main`.
  Without the `needs:` ordering, the push that merges the Release PR makes both start from the same
  pre-publish state, and the PR job re-proposes the version the release job is mid-publish on,
  producing a duplicate changelog entry. See release-plz#1542.
- **The Release PR needs a formatting fixup.** release-plz's changelog splice does not land on
  prettier's blank-line normalization, so the workflow runs the formatters to a fixed point and
  pushes a fixup commit. The loop is deliberate: prettier is not reliably idempotent in one pass
  over verbatim commit-body markdown.
- **`cliff.toml`'s skip-changelog matcher is `remote.pr_labels`.** Not `github.pr_labels`, which is
  what plain git-cliff exposes. The wrong path does not error; it silently empties the changelog.
