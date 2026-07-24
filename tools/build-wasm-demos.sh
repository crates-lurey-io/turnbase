#!/usr/bin/env bash
# Builds each reference game's browser demo and packages it into
# target/doc/demos/<game>/ (so it ships alongside the rustdoc + coverage on
# Pages), plus a gallery index.
#
# One game at a time, the retroglyph way: `cargo build --example <game>
# --features <game>` produces a wasm module containing exactly that game (see
# crates/demos), which wasm-bindgen then wraps. There is no aggregator crate
# and no runtime dispatch.
#
# Requires: the wasm32-unknown-unknown target and a wasm-bindgen CLI matching
# the wasm-bindgen version pinned in crates/demos/Cargo.toml.
#
# Usage: tools/build-wasm-demos.sh [output-dir]
#   output-dir defaults to target/doc/demos

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:-$repo_root/target/doc/demos}"
templates="$repo_root/docs/demos"

# Order and labels for the gallery cards.
games=(coup risk minion_battle woodland blackjack)
titles=(Coup Risk "Minion Battle" Woodland Blackjack)

cards=""
for i in "${!games[@]}"; do
  game="${games[$i]}"
  title="${titles[$i]}"
  echo "== $game =="

  cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    -p turnbase-demos \
    --example "$game" \
    --features "$game" \
    --target wasm32-unknown-unknown \
    --release

  mkdir -p "$out_dir/$game"
  wasm-bindgen \
    --target web \
    --out-dir "$out_dir/$game" \
    --out-name "$game" \
    "$repo_root/target/wasm32-unknown-unknown/release/examples/$game.wasm"

  sed -e "s/__GAME__/$game/g" -e "s/__TITLE__/$title/g" \
    "$templates/game-template.html" > "$out_dir/$game/index.html"
  cards="$cards<a class=\"card\" href=\"./$game/\"><h2>$title &rarr;</h2></a>\n"
done

# Plain-string substitution rather than sed: the accumulated cards HTML spans
# multiple lines once expanded.
cards_expanded="$(printf '%b' "$cards")"
template="$(cat "$templates/index-template.html")"
echo "${template//__CARDS__/$cards_expanded}" > "$out_dir/index.html"

echo "Wrote $out_dir (one self-contained module per game + gallery index)."
