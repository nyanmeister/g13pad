#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Collect cached crates' own notices for the locked Linux dependency graph.
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:?usage: collect-notices.sh OUTPUT_DIRECTORY}
[ ! -e "$output" ] || { echo 'Output must not already exist' >&2; exit 1; }
mkdir -p "$output"
meta=$(mktemp)
list=$(mktemp)
trap 'rm -f "$meta" "$list"' EXIT
platform=$(rustc -vV | sed -n 's/^host: //p')
cargo metadata --manifest-path "$repo/Cargo.toml" --locked --offline --filter-platform "$platform" --format-version 1 > "$meta"
jq -r '.packages | sort_by(.name,.version)[] | select(.source != null) | [.name,.version,(.license//"SEE LICENSE FILE"),.manifest_path] | @tsv' "$meta" > "$list"
cut -f1-3 "$list" > "$output/INDEX.tsv"
tab=$(printf '\t')
while IFS="$tab" read -r name version expression manifest; do
  crate_dir=$(dirname "$manifest")
  dest="$output/$name-$version"
  mkdir -p "$dest"
  rg --files --hidden "$crate_dir" -g '*LICENSE*' -g '*COPYING*' -g '*NOTICE*' -g '*COPYRIGHT*' -g '*license*' -g '*copyright*' |
  while IFS= read -r notice; do
    relative=${notice#"$crate_dir/"}
    mkdir -p "$dest/$(dirname "$relative")"
    cp "$notice" "$dest/$relative"
  done
  if [ "$name" = epaint_default_fonts ]; then
    mkdir -p "$dest/fonts"
    cp "$crate_dir"/fonts/*.txt "$dest/fonts/"
  fi
  printf '%s\n' "$expression" > "$dest/LICENSE-EXPRESSION.txt"
done < "$list"
