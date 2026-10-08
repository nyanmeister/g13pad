#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Isolated GUI selection/capture/live-refresh check. No input reaches the real desktop.
set -eu
if [ "${1:-}" = '--version' ]; then
 printf '%s\n' 'g13pad-layout-gui-check 0.2.0'
 exit 0
fi
if [ "$#" -ne 1 ]; then printf '%s\n' 'usage: check-layout-gui.sh OUTPUT_DIRECTORY' >&2; exit 2; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-"$repo/build/rust/release/g13map"}
out=$(realpath "$1")
fixture=$(mktemp -d /tmp/g13pad-live-layout.XXXXXX)
app_pid='' reader_pid='' wm_pid='' display_pid=''
cleanup() {
 for pid in "$app_pid" "$reader_pid" "$wm_pid" "$display_pid"; do
  [ -z "$pid" ] || { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
 done
 rm -rf "$fixture"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1200x900x24 -nolisten tcp -noreset 3>"$fixture/display" >"$out/layout-gui-xvfb.log" 2>&1 &
display_pid=$!
i=0
while [ ! -s "$fixture/display" ]; do kill -0 "$display_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
DISPLAY=:$(cat "$fixture/display")
export DISPLAY LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY
export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe" G13MAP_OUT_PIPE="$fixture/out" G13MAP_ANALOG=0 G13MAP_UNIT=0 G13MAP_ANALOG_MAP="$fixture/analog.map" XDG_RUNTIME_DIR="$fixture/runtime" I3SOCK="$fixture/absent-i3.sock"
mkdir -p "$G13MAP_CONFIG/profiles" "$XDG_RUNTIME_DIR"
printf 'font pango:monospace 10\nipc-socket %s/i3.sock\nfor_window [class="g13map"] floating enable\n' "$fixture" >"$fixture/i3.conf"
i3 -c "$fixture/i3.conf" -a >"$out/layout-gui-i3.log" 2>&1 &
wm_pid=$!
mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$out/layout-gui-fifo.log" &
reader_pid=$!
printf 'default\n' >"$G13MAP_CONFIG/active"
printf '# stick keys\nbind G1 KEY_D\nbind G2 KEY_E\nbind G3 KEY_LEFTCTRL+KEY_D KEY_E\n' >"$G13MAP_CONFIG/profiles/default.bind"
setxkbmap -layout us -variant norman -option ''
"$binary" --version >"$out/layout-gui-version.txt"
"$binary" edit >"$out/layout-gui.log" 2>&1 &
app_pid=$!
win=$(timeout 15 xdotool search --sync --class '^g13map$')
xdotool windowmap --sync "$win"
xdotool windowfocus --sync "$win"
sleep .5
printf '%s\n' "$DISPLAY" "$fixture" "$win" >"$out/layout-gui-state.txt"
magick import -display "$DISPLAY" -window "$win" "$out/layout-gui-initial.png"
snapshot() {
 magick import -display "$DISPLAY" -window "$win" "$out/layout-gui-$1.png"
}
# G2 starts as KEY_E (Norman D). Picking Norman E must change it to physical KEY_D.
xdotool mousemove --window "$win" 137 260 click 1
sleep .2
xdotool mousemove --window "$win" 770 227 click 1 type --clearmodifiers 'KEY_D'
sleep .3
snapshot picker
tesseract "$out/layout-gui-picker.png" "$out/layout-gui-picker" 2>/dev/null
rg -q 'E +KEY[._]D$' "$out/layout-gui-picker.txt"
xdotool mousemove --window "$win" 710 249 click 1
sleep .2
xdotool key --clearmodifiers ctrl+s
sleep .2
rg -q '^bind G2 KEY_D$' "$G13MAP_CONFIG/profiles/default.bind"
# Capture the physical D key, which XKB names e under Norman, on a different control.
xdotool mousemove --window "$win" 194 260 click --repeat 2 --delay 100 1
sleep .2
xdotool key --clearmodifiers e
sleep .2
xdotool key --clearmodifiers ctrl+s
sleep .2
rg -q '^bind G3 KEY_D$' "$G13MAP_CONFIG/profiles/default.bind"
cp "$G13MAP_CONFIG/profiles/default.bind" "$out/layout-gui-selected.bind"
# Keep the same editor alive during replacements and group changes.
for layout in de ru gr il; do
 setxkbmap -layout "$layout" -variant '' -option ''
 sleep 2.3
 "$binary" layout >"$out/layout-gui-$layout.json"
 snapshot "$layout"
done
c++ "$repo/tools/xkb-group.cpp" -lX11 -o "$fixture/group"
setxkbmap -layout us,ru -option ''
"$fixture/group" 1
sleep 2.3
snapshot group-1
"$fixture/group" 0
sleep 2.3
snapshot group-0
cmp "$G13MAP_CONFIG/profiles/default.bind" "$out/layout-gui-selected.bind"
capture_key() {
 layout=$1 symbol=$2 physical=$3
 setxkbmap -layout "$layout" -variant '' -option ''
 sleep 2.3
 xdotool mousemove --window "$win" 194 260 click --repeat 2 --delay 100 1
 sleep .2
 xdotool key --clearmodifiers "$symbol"
 sleep .2
 xdotool mousemove --window "$win" 860 15 click 1
 sleep .2
 rg -q "^bind G3 KEY_$physical\$" "$G13MAP_CONFIG/profiles/default.bind"
 snapshot "capture-$symbol"
}
capture_key de udiaeresis LEFTBRACE
capture_key de dead_acute EQUAL
capture_key ru Cyrillic_ve D
capture_key gr Greek_delta D
capture_key il hebrew_gimel D
cp "$G13MAP_CONFIG/profiles/default.bind" "$out/layout-gui-captured.bind"
printf '%s\n' 'Norman picker/capture/save, German punctuation/dead-key capture, Cyrillic/Greek/Hebrew capture, four live layout replacements and both groups passed. Live-refresh screenshots require visual review.'
