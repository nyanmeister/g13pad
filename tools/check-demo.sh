#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Real CLI/helper/watcher lifecycle on private FIFOs, with no desktop or pad input.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-demo-check 0.2.82'; exit 0; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${1:-"$repo/build/rust/release/g13map"}
fixture=$(mktemp -d /tmp/g13pad-demo.XXXXXX)
reader_pid='' watcher_pid='' demo_pid=''
stop() {
  [ -n "$1" ] || return 0
  kill "$1" 2>/dev/null || true
  for _try in $(seq 1 30); do kill -0 "$1" 2>/dev/null || break; sleep .05; done
  if kill -0 "$1" 2>/dev/null; then kill -KILL "$1" 2>/dev/null || true; fi
  wait "$1" 2>/dev/null || true
}
cleanup() { stop "$demo_pid"; stop "$watcher_pid"; stop "$reader_pid"; }
trap cleanup EXIT HUP INT TERM
export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe" G13MAP_OUT_PIPE="$fixture/out"
export G13MAP_ANALOG=0 G13MAP_UNIT=0 G13MAP_ANALOG_MAP="$fixture/analog.map"
export XDG_RUNTIME_DIR="$fixture/runtime" XDG_STATE_HOME="$fixture/state"
export G13MAP_DAEMON_CONFIG="$repo/packaging/default.bind" I3SOCK="$fixture/no-i3.sock"
unset DISPLAY WAYLAND_DISPLAY
mkdir -p "$XDG_RUNTIME_DIR" "$G13MAP_CONFIG/lcd"
chmod 700 "$XDG_RUNTIME_DIR"
perl -e 'print chr(3) x 960' > "$G13MAP_CONFIG/lcd/original.lpbm"
perl -e 'print chr(7) x 960' > "$G13MAP_CONFIG/lcd/changed.lpbm"
perl -e 'print chr(16) x 960' > "$fixture/demo.lpbm"
perl -e 'print pack("V",100),chr(16)x960,pack("V",100),chr(32)x960' > "$fixture/demo.anim"
printf '%s' 'R0lGODlhoAArAPAAAAAAAAAAACH/C05FVFNDQVBFMi4wAwEAAAAh+QQACgAAACwAAAAAoAArAAACXISPqcvtD6OctNqLs968+w+G4kiW5omm6sq27gvH8kzX9o3n+s73/g8MCofEovGITCqXzKbzCY1Kp9Sq9YrNarfcrvcLDovH5LL5jE6r1+y2+w2Py+f0uv2OFxcAACH5BAAKAAAALAAAAACgACsAgP///wAAAAJchI+py+0Po5y02ouz3rz7D4biSJbmiabqyrbuC8fyTNf2jef6zvf+DwwKh8Si8YhMKpfMpvMJjUqn1Kr1is1qt9yu9wsOi8fksvmMTqvX7Lb7DY/L5/S6/Y4XFwAAOw==' | base64 -d > "$fixture/test.gif"
original=$(md5sum "$G13MAP_CONFIG/lcd/original.lpbm" | cut -d' ' -f1)
changed=$(md5sum "$G13MAP_CONFIG/lcd/changed.lpbm" | cut -d' ' -f1)
demo=$(md5sum "$fixture/demo.lpbm" | cut -d' ' -f1)
pending="$XDG_RUNTIME_DIR/g13map-pipe.demo-pending"
"$binary" --version
"$binary" demo > "$fixture/usage"
rg -q 'default: 10' "$fixture/usage"
for seconds in 0 -1 NaN inf bad; do
  if "$binary" demo "$fixture/test.gif" "$seconds" > "$fixture/invalid" 2>&1; then exit 1; fi
done
test ! -e "$pending"
"$binary" profile create original
"$binary" profile lcd original original
"$binary" profile create changed
"$binary" profile lcd changed changed
mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$fixture/frames" &
reader_pid=$!
wait_frame() {
  for _try in $(seq 1 80); do
    if [ -f "$fixture/frames" ] && tail -n 1 "$fixture/frames" | rg -q "FRAME $1$"; then return 0; fi
    sleep .025
  done
  cat "$fixture/frames"
  return 1
}
for _try in $(seq 1 80); do [ ! -f "$fixture/frames" ] || break; sleep .025; done
"$binary" use original
wait_frame "$original"
# Exercise the packaged lean CLI's exec into the image decoder, with no DISPLAY.
"$binary" demo "$fixture/test.gif" .6 > "$fixture/gif.log" 2>&1
wait_frame "$original"
test ! -e "$pending"
"$binary" watch > "$fixture/watch.log" 2>&1 &
watcher_pid=$!
sleep .15
# Default duration is tested on the actual executable, not merely its parser.
start=$(date +%s)
"$binary" demo "$fixture/demo.lpbm" > "$fixture/default.log" 2>&1
elapsed=$(($(date +%s) - start))
[ "$elapsed" -ge 9 ] && [ "$elapsed" -le 14 ]
wait_frame "$original"
# Signals, competing demos, and SIGKILL recovery of a static active profile.
for signal in INT TERM KILL; do
  "$binary" demo "$fixture/demo.lpbm" 20 > "$fixture/$signal.log" 2>&1 &
  demo_pid=$!
  wait_frame "$demo"
  test -e "$pending"
  if "$binary" demo --clear > "$fixture/busy-clear" 2>&1; then exit 1; fi
  if "$binary" demo "$fixture/demo.lpbm" .1 > "$fixture/busy-demo" 2>&1; then exit 1; fi
  kill "-$signal" "$demo_pid"
  if [ "$signal" = KILL ]; then
    wait "$demo_pid" 2>/dev/null && exit 1
  else
    wait "$demo_pid"
  fi
  demo_pid=
  wait_frame "$original"
  test ! -e "$pending"
done
# A switch during the demo must restore the new profile rather than the old snapshot.
"$binary" demo "$fixture/demo.lpbm" 20 > "$fixture/switch.log" 2>&1 &
demo_pid=$!
wait_frame "$demo"
"$binary" use changed
kill -TERM "$demo_pid"
wait "$demo_pid"
demo_pid=
wait_frame "$changed"
# Recovery without a watcher is explicit and leaves saved configuration untouched.
stop "$watcher_pid"
watcher_pid=
"$binary" demo "$fixture/demo.lpbm" 20 > "$fixture/manual.log" 2>&1 &
demo_pid=$!
wait_frame "$demo"
kill -KILL "$demo_pid"
wait "$demo_pid" 2>/dev/null && exit 1
demo_pid=
test -e "$pending"
"$binary" demo --clear
wait_frame "$changed"
test ! -e "$pending"
# Every daemon read carrying a frame was a complete, unmerged 960-byte transfer.
test "$(rg -c ' FRAME ' "$fixture/frames")" -gt 10
printf 'PASS: demo CLI, decoder, default duration, signals, force-quit recovery and profile restoration: %s\n' "$fixture"
