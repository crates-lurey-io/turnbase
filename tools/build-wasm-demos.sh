#!/usr/bin/env bash
# Builds the turnbase-web crate for wasm32, runs wasm-bindgen, and packages one
# playable HTML page per reference game plus a gallery index into
# target/doc/demos (so it ships alongside the rustdoc + coverage on Pages).
#
# One wasm module drives every game (dispatched by name in `Demo::new`), so
# there is a single build and a single wasm-bindgen invocation; the per-game
# pages just load it with a different game string.
#
# Requires: the wasm32-unknown-unknown target and a wasm-bindgen CLI matching
# the wasm-bindgen version pinned in crates/web/Cargo.toml.
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

cargo build \
  --manifest-path "$repo_root/Cargo.toml" \
  -p turnbase-web \
  --target wasm32-unknown-unknown \
  --release

mkdir -p "$out_dir/pkg"
wasm-bindgen \
  --target web \
  --out-dir "$out_dir/pkg" \
  --out-name turnbase_web \
  "$repo_root/target/wasm32-unknown-unknown/release/turnbase_web.wasm"

cards=""
for i in "${!games[@]}"; do
  game="${games[$i]}"
  title="${titles[$i]}"
  echo "== $game =="
  mkdir -p "$out_dir/$game"
  sed -e "s/__GAME__/$game/g" -e "s/__TITLE__/$title/g" \
    "$templates/game-template.html" > "$out_dir/$game/index.html"
  cards="$cards<a class=\"card\" href=\"./$game/\"><h2>$title &rarr;</h2></a>\n"
done

# Plain-string substitution rather than sed: the accumulated cards HTML spans
# multiple lines once expanded.
cards_expanded="$(printf '%b' "$cards")"
template="$(cat "$templates/index-template.html")"
echo "${template//__CARDS__/$cards_expanded}" > "$out_dir/index.html"

echo "Wrote $out_dir (pkg + one page per game + gallery index)."
