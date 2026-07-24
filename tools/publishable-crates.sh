#!/usr/bin/env bash
# Prints the workspace's shippable library crates, one JSON object per line:
#   {"name":"turnbase-simulator","lib":"turnbase_simulator","dir":"/abs/path","description":"..."}
#
# "Shippable" = published (publish is not `false`, i.e. cargo metadata's
# `publish` is null rather than `[]`) AND has a library target. That excludes
# every example game crate and the non-published demos harness (all
# publish = false), plus any bin-only crate. This is the single source of
# truth for which crates the docs site documents, lists in its crates table,
# and generates llms.txt for -- so all three stay in sync automatically as
# crates are added or removed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo metadata --no-deps --format-version=1 --manifest-path "$repo_root/Cargo.toml" |
  jq -c '
    .packages[]
    | select(.publish != [])
    | . as $pkg
    | (.targets[] | select(.kind[] == "lib") | .name) as $lib
    | {
        name,
        lib: $lib,
        dir: (.manifest_path | rtrimstr("/Cargo.toml")),
        description: (.description // "")
      }
  ' |
  # Stable order: by crate name, so generated tables/pages are deterministic.
  jq -s -c 'sort_by(.name) | .[]'
