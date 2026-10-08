#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Exercise the actual CLI and sibling renderer with private files/FIFO and no display.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-cli-check 0.2.16'; exit 0; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${1:-"$repo/build/rust/release/g13map"}
fixture=$(mktemp -d /tmp/g13pad-cli.XXXXXX)
reader_pid=
cleanup() {
  [ -z "$reader_pid" ] || { kill "$reader_pid" 2>/dev/null || true; wait "$reader_pid" 2>/dev/null || true; }
  rm -rf "$fixture"
}
trap cleanup EXIT
export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe" G13MAP_ANALOG=0 G13MAP_UNIT=0
export G13MAP_ANALOG_MAP="$fixture/analog.map" XDG_RUNTIME_DIR="$fixture/runtime"
# The daemon baseline the transitions diff against, with G1 bound as upstream's example
# config binds it: the saved-unbind contract below needs a key that was live to clear.
export G13MAP_DAEMON_CONFIG="$fixture/daemon-default.bind"
printf 'rgb 31 0 127\nbind G1 KEY_ENTER\nbind G2 KEY_V\nbind TOP KEY_7\nbind LEFT KEY_M\n' > "$G13MAP_DAEMON_CONFIG"
unset DISPLAY WAYLAND_DISPLAY
mkdir -p "$XDG_RUNTIME_DIR"
"$binary" --version
"$binary" --help | rg -q 'profile bind'
"$binary" profile create 'CLI profile'
"$binary" profile bind 'CLI profile' G1 KEY_D
"$binary" profile controller 'CLI profile' right r3 0 0 0
"$binary" profile rgb 'CLI profile' 15 20 25
"$binary" show 'CLI profile' | rg -q '^bind G1 KEY_D$'
if "$binary" use 'CLI profile' >"$fixture/failed.log" 2>&1; then exit 1; fi
test ! -f "$G13MAP_CONFIG/active"
mkfifo "$G13MAP_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$fixture/commands" raw &
reader_pid=$!
i=0
while [ ! -f "$fixture/commands" ]; do kill -0 "$reader_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
"$binary" use 'CLI profile'
"$binary" profiles | rg -q '^\* CLI profile$'
# Saved unbind must clear an already-live key even though the same file is now unbound.
"$binary" profile unbind 'CLI profile' G1
"$binary" use 'CLI profile'
i=0
while ! rg -q '^bind G1 KEY_RESERVED$' "$fixture/commands"; do i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
"$binary" marquee 'Text without a display' cli > "$fixture/text.log"
# The Source engine feeder: the page file prints, a folder without an engine is refused.
"$binary" health source-res | rg -q '^"Logitech G-15 Keyboard Layout"$'
"$binary" health source-res | rg -q 'G13 %\(localplayer\)m_iHealth%'
if "$binary" health source "$fixture" >"$fixture/source.log" 2>&1; then exit 1; fi
rg -q 'no bin/engine.so' "$fixture/source.log"
test -f "$G13MAP_CONFIG/lcd/cli.lpbm"
test "$(wc -c < "$G13MAP_CONFIG/lcd/cli.lpbm")" -eq 960
"$binary" profile lcd 'CLI profile' cli
"$binary" panel waybar | jq -e '.text == "G13·CLI profile" and .class == "connected"' > /dev/null
"$binary" panel xfce | rg -q "<txtclick>'.*g13map' edit</txtclick>"
"$binary" panel lxqt | rg -q '^text:.* icon:.* tooltip:'
"$binary" panel text | rg -q '^G13 · CLI profile · connected$'
"$binary" modes set 3 'CLI profile'
"$binary" focus set kitty 'CLI profile'
"$binary" modes show | rg -q '^3 CLI profile$'
"$binary" focus show | rg -q '^kitty'
# The delegated renderer accepts argv data, not shell text, and returns exactly one frame.
# shellcheck disable=SC2016 # Literal shell syntax must remain argv data.
"$(dirname "$binary")/g13map-editor" --error-frame 'bad $(command) `command` Unicode: λ' > "$fixture/error-frame"
test "$(wc -c < "$fixture/error-frame")" -eq 960
printf '%s\n' 'CLI, saved-unbind, panel contracts and headless renderer checks passed'
