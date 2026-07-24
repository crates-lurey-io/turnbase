default:
    @just --list

# ── Formatting ───────────────────────────────────────────────────────────────

format:
    cargo fmt --all -- --check

fmt-fix:
    cargo fmt --all

# ── Linting ──────────────────────────────────────────────────────────────────

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

lint-fix:
    cargo clippy --workspace --all-targets --all-features --fix

# ── Build ──────────────────────────────────────────────────────────────────

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

# ── Test ───────────────────────────────────────────────────────────────────

test *args:
    cargo nextest run --workspace {{args}}

test-doc *args:
    cargo test --workspace {{args}} --doc

test-all:
    just test --all-features
    just test-doc --all-features

# ── Coverage ─────────────────────────────────────────────────────────────────

coverage *args:
    cargo llvm-cov --workspace --open {{args}}

coverage-gen:
    cargo llvm-cov --workspace --lcov --output-path lcov.info

# ── Composite ────────────────────────────────────────────────────────────────

fix:
    just fmt-fix
    just lint-fix

check:
    just format
    just lint
