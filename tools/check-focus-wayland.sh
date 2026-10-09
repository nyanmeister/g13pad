#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Window rules on Wayland: `g13map focus windows` and a `g13map watch` switching profiles
# by focus under a headless wlroots compositor (labwc by default; G13PAD_TEST_COMPOSITOR
# names another: sway, wayfire, river-classic; niri nested with G13PAD_TEST_PARENT=sway).
# Private runtime directory, config and FIFOs; nothing reaches a live session. Focus is
# moved the way every compositor agrees on: a new window takes focus, and closing the
# focused window gives it back.
# Usage: check-focus-wayland.sh OUTPUT_DIRECTORY
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-focus-wayland-check 0.2.38'; exit 0; fi
[ "$#" -eq 1 ] || { printf '%s\n' 'usage: check-focus-wayland.sh OUTPUT_DIRECTORY' >&2; exit 2; }
out=$(mkdir -p "$1" && realpath "$1")
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-/usr/bin/g13map}
compositor=${G13PAD_TEST_COMPOSITOR:-labwc}
for tool in "$compositor" foot perl "$binary"; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 2; }
done
box=$(mktemp -d /tmp/g13pad-wfocus.XXXXXX)
pids=''
cleanup() {
  # shellcheck disable=SC2086 # the PIDs are a word list
  [ -z "$pids" ] || kill $pids 2>/dev/null || true
  rm -rf "$box"
}
trap cleanup EXIT INT TERM
fail() { echo "FAIL: $*" >&2; exit 1; }
show() { cat "$1" 2>/dev/null | tr '\n' '|'; }

# The watcher's private world: two profiles, a rule, stand-in daemon pipes, no i3, no X.
mkdir -p "$box/config/profiles" "$box/runtime" "$box/state"
chmod 700 "$box/runtime"
printf 'rgb 0 184 252\nbind G1 KEY_D\n' >"$box/config/profiles/default.bind"
printf 'rgb 255 0 0\nbind G1 KEY_E\n' >"$box/config/profiles/eyes.bind"
printf 'default\n' >"$box/config/active"
printf 'on\nEyes\teyes\n' >"$box/config/focus"
cp "$box/config/profiles/default.bind" "$box/daemon-default.bind"
mkfifo "$box/pipe" "$box/out"
perl "$repo/tools/fakedaemon.pl" "$box/pipe" "$out/commands.log" &
pids="$pids $!"
export G13MAP_CONFIG="$box/config" G13MAP_PIPE="$box/pipe" G13MAP_OUT_PIPE="$box/out" \
  XDG_RUNTIME_DIR="$box/runtime" XDG_STATE_HOME="$box/state" G13MAP_UNIT=0 \
  G13MAP_ANALOG=1 G13MAP_ANALOG_MAP="$box/analog.map" \
  G13MAP_DAEMON_CONFIG="$box/daemon-default.bind" I3SOCK="$box/absent-i3.sock"
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK

# No compositor yet: the list must say so rather than guess.
if "$binary" focus windows >"$out/no-compositor.txt" 2>&1; then
  fail "focus windows succeeded without a compositor"
fi

# A headless compositor of its own, its socket in the private runtime directory. One with
# no headless backend (niri) runs nested inside a headless parent named by
# G13PAD_TEST_PARENT (sway); the newest socket is then the nested one.
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
newest_socket() {
  ls -t "$box"/runtime/wayland-[0-9]* 2>/dev/null | while read -r s; do
    [ -S "$s" ] && { basename "$s"; break; }
  done
}
start_compositor() {
  case "$1" in
  sway)
    printf 'output HEADLESS-1 resolution 1024x768\n' >"$box/sway.conf"
    sway -c "$box/sway.conf" >"$out/$1.log" 2>&1 &
    ;;
  *) "$1" >"$out/$1.log" 2>&1 & ;;
  esac
  pids="$pids $!"
  before=$2
  for _ in $(seq 80); do
    sock=$(newest_socket)
    [ -n "$sock" ] && [ "$sock" != "$before" ] && break
    sleep 0.1
  done
  [ -n "$sock" ] && [ "$sock" != "$before" ] || fail "$1 did not come up: $(show "$out/$1.log")"
}
sock=''
if [ -n "${G13PAD_TEST_PARENT:-}" ]; then
  command -v "$G13PAD_TEST_PARENT" >/dev/null || { echo "missing: $G13PAD_TEST_PARENT" >&2; exit 2; }
  start_compositor "$G13PAD_TEST_PARENT" ""
  export WAYLAND_DISPLAY="$sock"
  unset WLR_BACKENDS
  sleep 0.5
fi
start_compositor "$compositor" "$sock"
export WAYLAND_DISPLAY="$sock"
sleep 0.5

# A terminal of the default app id, then the watcher, then one of its own app id.
foot >"$out/foot-plain.log" 2>&1 &
plain=$!
pids="$pids $plain"
for _ in $(seq 50); do "$binary" focus windows 2>/dev/null | grep -q '^\* foot' && break; sleep 0.1; done
"$binary" focus windows >"$out/windows.txt" 2>&1 || fail "focus windows failed: $(show "$out/windows.txt")"
grep -q '^\* foot' "$out/windows.txt" || fail "the plain terminal is not listed as focused: $(show "$out/windows.txt")"

"$binary" watch >"$out/watch.log" 2>&1 &
pids="$pids $!"
sleep 1
expect_active() {
  for _ in $(seq 40); do
    [ "$(cat "$G13MAP_CONFIG/active")" = "$1" ] && return 0
    sleep 0.1
  done
  fail "active profile is '$(cat "$G13MAP_CONFIG/active")', expected '$1' after $2; watch.log: $(show "$out/watch.log")"
}
foot --app-id Eyes >"$out/foot-eyes.log" 2>&1 &
eyes=$!
pids="$pids $eyes"
expect_active eyes "opening the Eyes terminal"
kill "$eyes"
expect_active default "closing it (focus returns to the plain terminal)"
foot --app-id Eyes >"$out/foot-eyes2.log" 2>&1 &
pids="$pids $!"
expect_active eyes "opening Eyes again"
grep -q "^window 'Eyes'" "$out/watch.log" || fail "the watcher never named the Eyes window: $(show "$out/watch.log")"
echo "ok: $compositor on $sock; focus windows listed the terminal; the watcher switched eyes, default, eyes; output in $out"
