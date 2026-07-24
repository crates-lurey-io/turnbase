#!/usr/bin/env bash
# Generates the crates table page (target/doc/crates/index.html) from the
# shippable-crate list (tools/publishable-crates.sh) and the HTML template in
# docs/crates/index-template.html, so the table never drifts from the crates
# that actually ship.
#
# Each row links to the crate's rustdoc plus its generated llms-full.txt /
# llms.txt (produced by tools/gen-llms-txt.sh into the same target/doc/<lib>/
# directory).
#
# Usage: tools/gen-crates-index.sh [output-dir]
#   output-dir defaults to target/doc
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:-$repo_root/target/doc}"
template="$repo_root/docs/crates/index-template.html"

rows=""
while read -r crate; do
  [ -n "$crate" ] || continue
  name=$(jq -r '.name' <<<"$crate")
  lib=$(jq -r '.lib' <<<"$crate")
  desc=$(jq -r '.description' <<<"$crate")
  rows="$rows<tr><td class=\"crate\">$name</td><td class=\"desc\">$desc</td>"
  rows="$rows<td><a href=\"../$lib/index.html\">rustdoc</a></td>"
  rows="$rows<td><a href=\"../$lib/llms-full.txt\">llms-full.txt</a></td>"
  rows="$rows<td><a href=\"../$lib/llms.txt\">llms.txt</a></td></tr>\n"
done < <("$repo_root/tools/publishable-crates.sh")

# Plain-string substitution rather than sed: the accumulated rows span many
# lines once expanded, which sed's s/// chokes on past a handful of rows.
rows_expanded="$(printf '%b' "$rows")"
body="$(cat "$template")"
mkdir -p "$out_dir/crates"
echo "${body//__ROWS__/$rows_expanded}" > "$out_dir/crates/index.html"

echo "Wrote $out_dir/crates/index.html."
