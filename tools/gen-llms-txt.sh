#!/usr/bin/env bash
# Generates llms.txt / llms-full.txt for every shippable library crate, plus a
# workspace-level pair for the landing page.
#
# cargo-llms-txt reads a crate's Cargo.toml directly and chokes on workspace
# inheritance (`authors.workspace = true` etc.), so it can't be pointed at a
# member crate as-is. We resolve each crate's metadata with `cargo metadata`,
# materialize a self-contained Cargo.toml + README in a scratch directory
# alongside a symlinked `src/`, and run cargo-llms-txt there instead.
#
# Which crates count as shippable is decided by tools/publishable-crates.sh
# (the same list the docs site documents and tabulates), so this never drifts.
#
# Output layout matches `cargo doc`'s per-crate directories so the site can
# serve both from the same target/doc/<lib>/ folder:
#   target/doc/<lib>/llms.txt
#   target/doc/<lib>/llms-full.txt
#   target/doc/llms.txt           (workspace overview, from the root README)
#   target/doc/llms-full.txt
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

# Prefer a repo-local pinned binary if one was installed, else whatever
# `cargo llms-txt` resolves on PATH (CI installs it with cargo-binstall).
llms_txt_bin="$root_dir/.bin/cargo-llms-txt"
if [ ! -x "$llms_txt_bin" ]; then
  llms_txt_bin="cargo llms-txt"
fi

out_dir="${1:-target/doc}"
mkdir -p "$out_dir"

# Workspace-level overview (root README + workspace Cargo.toml), used by the
# docs landing page and unrelated to any single crate's API surface.
$llms_txt_bin >/dev/null
cp llms.txt llms-full.txt "$out_dir/"

work_root="$(mktemp -d)"
trap 'rm -rf "$work_root"' EXIT

# Same publish-filter as tools/publishable-crates.sh, but pulling the extra
# Cargo metadata (authors, license, edition, ...) the materialized manifest
# needs. Keeping the filter identical keeps the two in lockstep.
cargo metadata --no-deps --format-version=1 |
  jq -c '.packages[] | select(.publish != []) | {
    name, description, authors, license, repository, keywords, categories,
    rust_version, edition,
    manifest_dir: (.manifest_path | rtrimstr("/Cargo.toml")),
    lib_name: (.targets[] | select(.kind[] == "lib") | .name)
  }' | while read -r pkg; do
  name=$(jq -r '.name' <<<"$pkg")
  manifest_dir=$(jq -r '.manifest_dir' <<<"$pkg")
  lib_name=$(jq -r '.lib_name // empty' <<<"$pkg")
  [ -n "$lib_name" ] || continue # skip crates with no library target

  scratch="$work_root/$name"
  mkdir -p "$scratch"
  ln -s "$manifest_dir/src" "$scratch/src"

  jq -r '
    "[package]",
    "name = " + (.name | tojson),
    "version = \"0.0.0\"",
    "edition = " + (.edition | tojson),
    "description = " + (.description // "" | tojson),
    "license = " + (.license // "" | tojson),
    "repository = " + (.repository // "" | tojson),
    "keywords = " + (.keywords | tojson),
    "categories = " + (.categories | tojson),
    "authors = " + (.authors | tojson)
  ' <<<"$pkg" >"$scratch/Cargo.toml"

  description=$(jq -r '.description // ""' <<<"$pkg")
  cat >"$scratch/README.md" <<EOF
# $name

$description

Part of the [turnbase](https://github.com/crates-lurey-io/turnbase) workspace.
EOF

  (cd "$scratch" && $llms_txt_bin >/dev/null)

  crate_out="$out_dir/$lib_name"
  mkdir -p "$crate_out"
  cp "$scratch/llms.txt" "$scratch/llms-full.txt" "$crate_out/"
done

echo "Wrote $out_dir/llms.txt and per-crate llms.txt/llms-full.txt."
