default:
    @just --list

# ── Formatting ───────────────────────────────────────────────────────────────

rustfmt:
    cargo fmt --all -- --check

# Probe for the installed binary, not just tools/node_modules: a partial or pruned tree (e.g. left
# behind after a branch switch that removed the ignore rule) satisfies a directory test but has no
# .bin, so the install gets skipped and the recipe fails with "command not found".
prettier:
    @[ -x tools/node_modules/.bin/prettier ] || npm ci --prefix tools
    npm --prefix tools run format:check

markdown:
    @[ -x tools/node_modules/.bin/markdownlint-cli2 ] || npm ci --prefix tools
    npm --prefix tools run lint

# Auto-fix Rust + Markdown/YAML/JSON formatting.
fmt:
    cargo fmt --all
    @[ -x tools/node_modules/.bin/prettier ] || npm ci --prefix tools
    npm --prefix tools run format

# Check-only counterpart of `fmt` (what CI's format job runs).
fmt-check: rustfmt prettier

# ── Linting ──────────────────────────────────────────────────────────────────

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

clippy-fix:
    cargo clippy --workspace --all-targets --all-features --fix

lint: clippy markdown

# ── Build ────────────────────────────────────────────────────────────────────

compile:
    cargo check --workspace --all-targets --all-features

doc:
    cargo doc --workspace --no-deps --document-private-items --all-features

doc-gen:
    cargo clean --doc
    cargo doc --workspace --no-deps --all-features
    echo '<meta http-equiv="refresh" content="0;url=turnbase/index.html">' > target/doc/index.html
    rm -f target/doc/.lock

# The full GitHub Pages site into target/doc: rustdoc API docs (shippable
# libraries only), per-crate + workspace llms.txt, a crates index table, a
# self-hosted per-line HTML coverage report under coverage/, the WASM demos,
# and the landing page. This is exactly what the Docs workflow ships to Pages,
# so it can be previewed locally (serve target/doc over HTTP and open
# index.html).
#
# Only the shippable library crates are documented (see
# tools/publishable-crates.sh): the example game crates and the non-published
# demos harness are publish = false, so their rustdoc has no business on the
# public docs site.

# Build the full GitHub Pages site into target/doc.
docs-site:
    cargo doc --no-deps --all-features $(tools/publishable-crates.sh | jq -r '"-p " + .name' | tr '\n' ' ')
    tools/gen-llms-txt.sh target/doc
    tools/gen-crates-index.sh target/doc
    rm -rf target/doc/coverage
    cargo llvm-cov --workspace --all-features --html --output-dir target/doc/coverage
    mv target/doc/coverage/html/* target/doc/coverage/
    rmdir target/doc/coverage/html
    tools/build-wasm-demos.sh
    cp docs/site/index.html target/doc/index.html
    sed -i.bak "s/__GIT_SHA__/$(git rev-parse --short HEAD 2>/dev/null || echo unknown)/g" target/doc/index.html && rm -f target/doc/index.html.bak
    rm -f target/doc/.lock

# ── Test ─────────────────────────────────────────────────────────────────────

# nextest runs every test in its own process, in parallel across all of them. It does not run
# doctests (https://nexte.st/docs/limitations/), so `test-doc` covers those separately. See
# .config/nextest.toml for the profile config.
#
# (Blank line below is load-bearing: `just` uses the comment block directly above a recipe as its
# `--list` doc string and shows only the last line, so a long rationale renders as a fragment.)

# Run all tests except doctests.
test *args:
    cargo nextest run --workspace {{ args }}

test-doc *args:
    cargo test --workspace {{ args }} --doc

test-all:
    just test --all-features
    just test-doc --all-features

# CI variant: same tests, but under the `ci` nextest profile, which additionally writes JUnit XML
# to target/nextest/ci/junit.xml for Codecov Test Analytics (see the `test` job in ci.yml).

# Run all tests under the `ci` profile, emitting JUnit XML.
test-ci:
    cargo nextest run --workspace --all-features --profile ci
    cargo test --workspace --all-features --doc

# ── Dependencies ─────────────────────────────────────────────────────────────

# Advisories are soft-failed in CI (a new RUSTSEC advisory against a transitive dep shouldn't
# turn every unrelated PR red), while licenses/bans/sources are a hard gate.

# Check RUSTSEC advisories (soft-failed in CI).
deny-advisories:
    cargo deny check advisories

deny-licenses:
    cargo deny check bans licenses sources

deny: deny-advisories deny-licenses

# ── Coverage ─────────────────────────────────────────────────────────────────

coverage *args:
    cargo llvm-cov --workspace --open {{ args }}

# cargo-llvm-cov writes absolute `SF:` paths. Codecov can usually match those against the repo's
# file list, but relying on that heuristic means codecov.yml's per-crate `paths:` silently match
# nothing and every flag reports 0%. Rewrite them to repo-relative so the match is literal.
# sed -i.bak + rm is the portable form; bare `sed -i` differs between BSD and GNU.

# Generate lcov.info for Codecov.
coverage-gen:
    cargo llvm-cov --workspace --lcov --output-path lcov.info
    sed -i.bak "s|^SF:$(pwd)/|SF:|" lcov.info && rm -f lcov.info.bak

# ── Composite ────────────────────────────────────────────────────────────────

fix:
    just fmt
    just clippy-fix

# The full local gate. `compile` is deliberately not included: clippy already performs a
# strictly-stronger typecheck than plain `cargo check`, and `test` right after does a full build,
# so a standalone check pass between them never catches anything those two don't. `just compile`
# stays available on its own for a fast check-only iteration loop.

# Full local gate: fmt-check, lint, test, doc. Run before every commit.
check: fmt-check lint test-all doc

clean:
    cargo clean
