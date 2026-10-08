#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compare installed/extracted executables using copied profiles and a private display.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-local-comparison 0.2.29'; exit 0; fi
[ "$#" -eq 4 ] || { printf '%s\n' 'usage: check-local-comparison.sh OLD_BIN_DIR NEW_BIN_DIR CONFIG_COPY OUTPUT' >&2; exit 2; }
old=$(realpath "$1") new=$(realpath "$2") config=$(realpath "$3")
mkdir -p "$4"
out=$(realpath "$4")
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d /tmp/g13pad-comparison.XXXXXX)
display_pid='' wm_pid='' app_pid='' reader_pid=''
cleanup() {
  for pid in "$app_pid" "$reader_pid" "$wm_pid" "$display_pid"; do
    [ -z "$pid" ] || { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
  done
  rm -rf "$fixture"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1400x1050x24 -nolisten tcp -noreset 3>"$fixture/display" >"$out/xvfb.log" 2>&1 &
display_pid=$!
i=0
while [ ! -s "$fixture/display" ]; do kill -0 "$display_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
DISPLAY=:$(cat "$fixture/display")
export DISPLAY LIBGL_ALWAYS_SOFTWARE=1
export XDG_RUNTIME_DIR="$fixture/runtime" G13MAP_CONFIG="$fixture/config"
export G13MAP_PIPE="$fixture/pipe" G13MAP_OUT_PIPE="$fixture/out" G13MAP_ANALOG=0 G13MAP_UNIT=0
export G13MAP_ANALOG_MAP="$fixture/analog.map" I3SOCK="$fixture/absent.sock"
unset WAYLAND_DISPLAY
mkdir -p "$XDG_RUNTIME_DIR"
printf 'font pango:monospace 10\nfor_window [class="g13map"] floating enable\n' > "$fixture/i3.conf"
i3 -c "$fixture/i3.conf" -a >"$out/i3.log" 2>&1 &
wm_pid=$!
setxkbmap -layout us -variant norman -option ''
for version in old new; do
  case "$version" in old) binary="$old/g13map";; new) binary="$new/g13map";; esac
  cp -a "$config" "$G13MAP_CONFIG"
  "$binary" --version > "$out/$version-version.txt"
  "$binary" layout | jq -S . > "$out/$version-layout.json"
  "$binary" > "$out/$version-panel.txt"
  : > "$out/$version-polls.tsv"
  i=0
  while [ "$i" -lt 30 ]; do
    /usr/bin/time -f '%M\t%e' -o "$out/time" "$binary" > /dev/null
    cat "$out/time" >> "$out/$version-polls.tsv"
    i=$((i+1))
  done
  mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
  perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$out/$version-commands.log" &
  reader_pid=$!
  i=0
  while [ ! -f "$out/$version-commands.log" ]; do kill -0 "$reader_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
  "$binary" edit > "$out/$version-editor.log" 2>&1 &
  app_pid=$!
  win=$(timeout 15 xdotool search --sync --class '^g13map$')
  xdotool windowmap --sync "$win"
  xdotool windowfocus --sync "$win"
  sleep .8
  magick import -display "$DISPLAY" -window "$win" "$out/$version-editor.png"
  editor_pid=$(xdotool getwindowpid "$win")
  ps -p "$editor_pid" -o pid,rss,args > "$out/$version-editor-rss.txt"
  kill "$app_pid"
  wait "$app_pid" 2>/dev/null || true
  app_pid=''
  kill "$reader_pid"
  wait "$reader_pid" 2>/dev/null || true
  reader_pid=''
  rm -rf "$G13MAP_CONFIG"
  rm -f "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
done
cmp "$out/old-layout.json" "$out/new-layout.json"
printf '%s\n' 'PASS: both copied-profile editors launched; Norman layout JSON identical; review screenshots and poll measurements.'
