#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Private Xvfb session: visual review + actual LCD/G-key/thumb controls at two sizes.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-board-check 0.2.16'; exit 0; fi
if [ "$#" -ne 1 ]; then printf '%s\n' 'usage: check-board-gui.sh OUTPUT_DIRECTORY' >&2; exit 2; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-"$repo/build/rust/release/g13map"}
mkdir -p "$1"
out=$(realpath "$1")
fixture=$(mktemp -d /tmp/g13pad-board.XXXXXX)
display_pid='' reader_pid='' app_pid='' wm_pid='' panel_pid=''
cleanup() {
  [ -z "$panel_pid" ] || kill -- "-$panel_pid" 2>/dev/null || true
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
unset WAYLAND_DISPLAY
export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe" G13MAP_OUT_PIPE="$fixture/out"
export G13MAP_ANALOG=0 G13MAP_UNIT=0 XDG_RUNTIME_DIR="$fixture/runtime" I3SOCK="$fixture/absent.sock"
mkdir -p "$G13MAP_CONFIG/profiles" "$XDG_RUNTIME_DIR"
printf 'font pango:monospace 10\nfor_window [class="g13map"] floating enable\n' > "$fixture/i3.conf"
i3 -c "$fixture/i3.conf" -a >"$out/i3.log" 2>&1 &
wm_pid=$!
setxkbmap -layout us -variant norman -option ''
mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$out/commands.log" &
reader_pid=$!
i=0
while [ ! -f "$out/commands.log" ]; do kill -0 "$reader_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
printf 'default\n' > "$G13MAP_CONFIG/active"
printf '# stick keys\nrgb 0 184 252\nbind G1 KEY_D\nbind G2 KEY_E\n' > "$G13MAP_CONFIG/profiles/default.bind"
"$binary" edit >"$out/editor.log" 2>&1 &
app_pid=$!
win=$(timeout 15 xdotool search --sync --class '^g13map$')
xdotool windowmap --sync "$win"
xdotool windowfocus --sync "$win"
sleep .7
magick import -display "$DISPLAY" -window "$win" "$out/board-normal.png"
# Verify key and sloping thumb-button hit regions, using only this display's pointer.
for selection in '145 265 G2' '448 558 DOWN'; do
  # shellcheck disable=SC2086 # Intentional split of fixed numeric test tuples.
  set -- $selection
  xdotool mousemove --window "$win" "$1" "$2" click 1
  sleep .2
  magick import -display "$DISPLAY" -window "$win" "$out/selected-$3.png"
  magick "$out/selected-$3.png" -crop 320x125+660+56 "$out/selection.png"
  tesseract "$out/selection.png" "$out/selection" 2>/dev/null
  rg -q "^$3" "$out/selection.txt"
done
xdotool windowsize --sync "$win" 760 700
sleep .5
magick import -display "$DISPLAY" -window "$win" "$out/board-minimum.png"
kill "$app_pid"
wait "$app_pid" 2>/dev/null || true
app_pid=''
# A native LXQt panel gets its own config, runtime directory, display and D-Bus session.
if command -v lxqt-panel >/dev/null; then
  export XDG_CONFIG_HOME="$fixture/xdg" XDG_CACHE_HOME="$fixture/cache"
  mkdir -p "$XDG_CONFIG_HOME/lxqt"
  cat > "$XDG_CONFIG_HOME/lxqt/panel.conf" <<CONFIG
[General]
__userfile__=true
[panel1]
plugins=customcommand1
position=Top
panelSize=32
length=100
alignment=-1
[customcommand1]
type=customcommand
command=$binary panel lxqt
click=$binary edit
runWithBash=false
outputFormat=2
repeatTimer=1
maxWidth=300
alignment=Left
CONFIG
  setsid dbus-run-session -- lxqt-panel > "$out/lxqt.log" 2>&1 &
  panel_pid=$!
  sleep 1.8
  magick import -display "$DISPLAY" -window root "$out/lxqt-connected.png"
  for x in 16 36; do
    xdotool mousemove "$x" 16 click 1
    win=$(timeout 10 xdotool search --sync --class '^g13map$')
    app_pid=$(xdotool getwindowpid "$win")
    xdotool windowmap --sync "$win"
    kill "$app_pid"
    app_pid=''
    sleep .3
  done
  kill "$reader_pid"
  wait "$reader_pid" 2>/dev/null || true
  reader_pid=''
  sleep 1.8
  magick import -display "$DISPLAY" -window root "$out/lxqt-disconnected.png"
fi
printf '%s\n' 'Board sizes/key/thumb hits and native LXQt icon/text clicks passed; inspect screenshots.'
