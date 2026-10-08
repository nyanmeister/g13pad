#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Reads key labels on a private X server; never changes the real session's layout/input.
set -eu
if [ "${1:-}" = '--version' ]; then
  printf '%s\n' 'g13pad-layout-check 0.2.0'
  exit 0
fi
if [ "$#" -ne 1 ]; then printf '%s\n' 'usage: check-layouts.sh OUTPUT_DIRECTORY' >&2; exit 2; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-"$repo/build/rust/release/g13map"}
out=$(realpath "$1")
fixture=$(mktemp -d /tmp/g13pad-layouts.XXXXXX)
c++ "$repo/tools/xkb-group.cpp" -lX11 -o "$fixture/group"
"$fixture/group" --version > "$out/layout-fixture-version.txt"
display_pid=
# shellcheck disable=SC2329
cleanup() {
  if [ -n "$display_pid" ]; then kill "$display_pid" 2>/dev/null || true; wait "$display_pid" 2>/dev/null || true; fi
  rm -rf "$fixture"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1000x800x24 -nolisten tcp -noreset 3>"$fixture/display" >"$out/layout-xvfb.log" 2>&1 &
display_pid=$!
attempt=0
while [ ! -s "$fixture/display" ]; do
  kill -0 "$display_pid"
  attempt=$((attempt + 1))
  [ "$attempt" -lt 50 ]
  sleep 0.1
done
DISPLAY=:$(cat "$fixture/display")
export DISPLAY
unset WAYLAND_DISPLAY
"$binary" --version > "$out/layout-version.txt"
check() {
  tag=$1 layout=$2 variant=$3 expected=$4
  setxkbmap -display "$DISPLAY" -layout "$layout" -variant "$variant" -option ''
  "$binary" layout > "$out/layout-$tag.json"
  jq -e "$expected" "$out/layout-$tag.json" >/dev/null
}
check us us '' '.D == "d" and .E == "e" and .LEFTBRACE == "["'
check norman us norman '.D == "e" and .E == "d"'
check dvorak us dvorak '.D == "e" and .E == "." and .Q == "\u0027"'
check colemak us colemak '.D == "s" and .E == "f"'
check french fr '' '.A == "q" and .Q == "a" and .W == "z" and .LEFTBRACE == "Dead circumflex"'
check german de '' '.Y == "z" and .Z == "y" and .LEFTBRACE == "ü" and .EQUAL == "Dead acute"'
check russian ru '' '.D == "в" and .E == "у"'
check greek gr '' '.D == "δ" and .E == "ε"'
check hebrew il '' '.D == "ג" and .E == "ק"'
setxkbmap -display "$DISPLAY" -layout us,ru -option '' -option grp:alt_shift_toggle
"$binary" layout > "$out/layout-group-0.json"
jq -e '.D == "d"' "$out/layout-group-0.json" >/dev/null
"$fixture/group" 1
"$binary" layout > "$out/layout-group-1.json"
jq -e '.D == "в"' "$out/layout-group-1.json" >/dev/null
"$fixture/group" 0
"$binary" layout > "$out/layout-group-return.json"
jq -e '.D == "d"' "$out/layout-group-return.json" >/dev/null
# Repeated replacements expose stale-map caching and X connection/resource leaks.
i=0
while [ "$i" -lt 30 ]; do
  check repeat-norman us norman '.D == "e" and .E == "d"'
  check repeat-russian ru '' '.D == "в" and .E == "у"'
  check repeat-us us '' '.D == "d" and .E == "e"'
  i=$((i + 1))
done
DISPLAY="$fixture/absent-display" "$binary" layout > "$out/layout-no-display.json"
jq -e 'length == 0' "$out/layout-no-display.json" >/dev/null
printf '%s\n' '9 layouts, punctuation/Unicode, both active groups, 90 repeated replacements, and no-display fallback passed.'
