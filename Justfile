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
