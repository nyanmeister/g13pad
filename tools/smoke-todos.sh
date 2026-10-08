#!/bin/sh
# Isolated display/FIFOs; no input or service calls reach the user's desktop.
# Usage: sh tools/smoke-todos.sh REVIEW_OUTPUT_DIRECTORY
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-"$repo/build/rust/release/g13map"}
out=$(realpath "$1")
fixture=$(mktemp -d /tmp/g13map-todo-gui.XXXXXX)
display_pid=
reader_pid=
app_pid=
wm_pid=
# shellcheck disable=SC2329
cleanup() {
  for pid in "$app_pid" "$reader_pid" "$wm_pid" "$display_pid"; do
    [ -z "$pid" ] || { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
  done
  rm -rf "$fixture"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1200x900x24 -nolisten tcp 3>"$fixture/display" >"$out/xvfb.log" 2>&1 &
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
export LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY
export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe"
export G13MAP_OUT_PIPE="$fixture/out" G13MAP_ANALOG=1 G13MAP_UNIT=0
export XDG_RUNTIME_DIR="$fixture/runtime" I3SOCK="$fixture/absent-i3.sock"
printf 'font pango:monospace 10\nipc-socket %s/i3.sock\nfor_window [class="g13map"] floating enable\n' "$fixture" > "$fixture/i3.conf"
i3 -c "$fixture/i3.conf" -a > "$out/i3.log" 2>&1 &
wm_pid=$!
mkdir -p "$G13MAP_CONFIG/profiles" "$XDG_RUNTIME_DIR"
mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$out/gui-fifo.log" &
reader_pid=$!
printf 'default\n' > "$G13MAP_CONFIG/active"
printf 'rgb 31 0 127\nbind G1 KEY_ENTER\nbind TOP KEY_C\n' > "$G13MAP_CONFIG/profiles/default.bind"
magick -size 80x80 xc:none -fill black -draw 'rectangle 0,0 39,79' "$fixture/source.png"
"$binary" edit >"$out/gui.log" 2>&1 &
app_pid=$!
win=$(timeout 15 xdotool search --sync --class '^g13map$')
# eframe starts hidden until its first render; mapping gives Xvfb an initial expose event.
xdotool windowmap --sync "$win"
sleep 0.4
magick import -display "$DISPLAY" -window "$win" "$out/01-editor.png"
printf '%s\n' "$win" > "$out/window-id"
xdotool mousemove --window "$win" 250 110 click 1
sleep 0.3
magick import -display "$DISPLAY" -window "$win" "$out/02-lcd-picker.png"
xdotool mousemove --window "$win" 690 230 click 1
sleep 0.3
magick import -display "$DISPLAY" -window "$win" "$out/03-text-dialog.png"
xdotool mousemove --window "$win" 150 100 click 1 type --clearmodifiers 'Error: profile unavailable'
sleep 0.5
xdotool mousemove --window "$win" 50 415 click 1
sleep 0.4
magick import -display "$DISPLAY" -window "$win" "$out/04-scrolling-text.png"
xdotool mousemove --window "$win" 855 15 click 1
sleep 0.2
rg -q '^# lcd text$' "$G13MAP_CONFIG/profiles/default.bind"
test -f "$G13MAP_CONFIG/lcd/text.text"
test -f "$G13MAP_CONFIG/lcd/text.anim"
cp "$G13MAP_CONFIG/lcd/text.lpbm" "$G13MAP_CONFIG/lcd/text.anim" "$G13MAP_CONFIG/lcd/text.textopts" "$out/"
# The selected kept text reopens with its message/settings. Make a multiline variant
# with a different installed font, still speed, wrap, inversion and right alignment.
xdotool mousemove --window "$win" 690 251 click 1
sleep 0.3
xdotool mousemove --window "$win" 150 100 click 1 key --clearmodifiers ctrl+a
xdotool type --clearmodifiers 'G13'
xdotool key --clearmodifiers Return
xdotool type --clearmodifiers 'Two lines'
xdotool mousemove --window "$win" 400 135 click 1
sleep 0.2
xdotool mousemove --window "$win" 130 288 click 1
xdotool mousemove --window "$win" 151 157 click --repeat 2 --delay 100 1 key --clearmodifiers ctrl+a type --clearmodifiers '16'
xdotool key --clearmodifiers Return
xdotool mousemove --window "$win" 151 178 click --repeat 2 --delay 100 1 key --clearmodifiers ctrl+a type --clearmodifiers '0'
xdotool key --clearmodifiers Return
xdotool mousemove --window "$win" 30 199 click 1
xdotool mousemove --window "$win" 174 199 click 1
xdotool mousemove --window "$win" 375 199 click 1
sleep 0.5
magick import -display "$DISPLAY" -window "$win" "$out/07-multiline-text-preview.png"
xdotool mousemove --window "$win" 50 415 click 1
sleep 0.4
xdotool mousemove --window "$win" 855 15 click 1
sleep 0.2
rg -q '^# lcd text-2$' "$G13MAP_CONFIG/profiles/default.bind"
rg -q '^size 16$' "$G13MAP_CONFIG/lcd/text-2.textopts"
rg -q '^speed 0$' "$G13MAP_CONFIG/lcd/text-2.textopts"
rg -q '^wrap 1$' "$G13MAP_CONFIG/lcd/text-2.textopts"
rg -q '^invert 1$' "$G13MAP_CONFIG/lcd/text-2.textopts"
rg -q '^align right$' "$G13MAP_CONFIG/lcd/text-2.textopts"
rg -q '^font /' "$G13MAP_CONFIG/lcd/text-2.textopts"
test "$(cat "$G13MAP_CONFIG/lcd/text-2.text")" = "$(printf 'G13\nTwo lines')"
test ! -f "$G13MAP_CONFIG/lcd/text-2.anim"
cp "$G13MAP_CONFIG/lcd/text-2.text" "$G13MAP_CONFIG/lcd/text-2.textopts" "$G13MAP_CONFIG/lcd/text-2.lpbm" "$out/"
xdotool mousemove --window "$win" 690 230 click 1
sleep 0.4
magick import -display "$DISPLAY" -window "$win" "$out/08-restored-text-settings.png"
xdotool mousemove --window "$win" 540 225 click 1 key --clearmodifiers Escape
sleep 0.2
xdotool getwindowname "$win" > "$out/editor-after-text-escape.txt"
magick import -display "$DISPLAY" -window "$win" "$out/09-text-escape.png"
kill "$app_pid"
wait "$app_pid" 2>/dev/null || true
app_pid=
printf 'rgb 31 0 127\nbind G1 KEY_ENTER\nbind TOP KEY_C\n' > "$G13MAP_CONFIG/profiles/default.bind"
"$binary" edit >>"$out/gui.log" 2>&1 &
app_pid=$!
win=$(timeout 15 xdotool search --sync --class '^g13map$')
xdotool windowmap --sync "$win"
sleep 0.4
xdotool mousemove --window "$win" 250 110 click 1
sleep 0.2
xdotool mousemove --window "$win" 800 250 click 1 type --clearmodifiers "$fixture/source.png"
xdotool mousemove --window "$win" 945 250 click 1
sleep 0.4
magick import -display "$DISPLAY" -window "$win" "$out/05-background-adjust.png"
xdotool mousemove --window "$win" 528 543
sleep 0.2
xdotool mousedown 1
sleep 0.2
xdotool mouseup 1
sleep 0.4
magick import -display "$DISPLAY" -window "$win" "$out/06-background-zero.png"
rg -q '^background 0$' "$G13MAP_CONFIG/lcd/source.conv"
cp "$G13MAP_CONFIG/lcd/source.conv" "$G13MAP_CONFIG/lcd/source.lpbm" "$out/"
"$binary" marquee 'CLI text with spaces' cli > "$out/marquee-cli.log"
test -f "$G13MAP_CONFIG/lcd/cli.text"
test -f "$G13MAP_CONFIG/lcd/cli.anim"
