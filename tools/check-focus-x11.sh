#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Window rules without i3: `g13map focus windows` and a `g13map watch` that switches
# profiles by focus on a private Xvfb display under an EWMH window manager (openbox by
# default; G13PAD_TEST_WM names another, e.g. xfwm4). Private config, FIFOs and runtime
# directory; nothing reaches the live desktop. Usage: check-focus-x11.sh OUTPUT_DIRECTORY
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-focus-x11-check 0.2.36'; exit 0; fi
[ "$#" -eq 1 ] || { printf '%s\n' 'usage: check-focus-x11.sh OUTPUT_DIRECTORY' >&2; exit 2; }
out=$(mkdir -p "$1" && realpath "$1")
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-/usr/bin/g13map}
wm=${G13PAD_TEST_WM:-openbox}
display=${G13PAD_TEST_DISPLAY:-:98}
for tool in Xvfb "$wm" xterm xdotool xprop perl "$binary"; do
  command -v "$tool" >/dev/null || { echo "missing: $tool" >&2; exit 2; }
done
box=$(mktemp -d /tmp/g13pad-focus.XXXXXX)
pids=''
cleanup() {
  # shellcheck disable=SC2086 # the PIDs are a word list
  [ -z "$pids" ] || kill $pids 2>/dev/null || true
  rm -rf "$box"
}
trap cleanup EXIT INT TERM
fail() { echo "FAIL: $*" >&2; exit 1; }
show() { cat "$1" 2>/dev/null | tr '\n' '|'; }

# A session of its own.
Xvfb "$display" -screen 0 1024x768x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
pids="$!"
export DISPLAY="$display" XAUTHORITY="$box/no-such-authority"
for _ in $(seq 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.1; done
xdotool getdisplaygeometry >/dev/null 2>&1 || fail "Xvfb did not come up on $display"

# The watcher's private world: two profiles, a rule, stand-in daemon pipes, no i3.
mkdir -p "$box/config/profiles" "$box/runtime" "$box/state"
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

# No window manager yet: the list must say so rather than guess.
if "$binary" focus windows >"$out/no-wm.txt" 2>&1; then
  fail "focus windows succeeded without a window manager"
fi
grep -q 'no EWMH window manager' "$out/no-wm.txt" \
  || fail "unexpected error without a window manager: $(show "$out/no-wm.txt")"

"$wm" >"$out/wm.log" 2>&1 &
pids="$pids $!"
for _ in $(seq 50); do
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' && break
  sleep 0.1
done
# Two xterms, the second given a class of its own, so the test needs one client program.
xterm -geometry 60x10+0+0 &
pids="$pids $!"
xterm -class Eyes -geometry 60x10+0+300 &
pids="$pids $!"
term=$(xdotool search --sync --class XTerm | head -1)
eyes=$(xdotool search --sync --class Eyes | head -1)
sleep 0.5

# The list: both windows, their classes, exactly one focused.
"$binary" focus windows >"$out/windows.txt"
grep -q 'XTerm' "$out/windows.txt" || fail "the plain xterm is missing: $(show "$out/windows.txt")"
grep -q 'Eyes' "$out/windows.txt" || fail "the Eyes xterm is missing: $(show "$out/windows.txt")"
focused=$(grep -c '^\*' "$out/windows.txt" || true)
[ "$focused" -eq 1 ] || fail "not exactly one focused window: $(show "$out/windows.txt")"

# Focus moves by both routes, since window managers differ: a _NET_ACTIVE_WINDOW request
# (windowactivate; what tiling managers such as i3 and bspwm honour) and then
# XSetInputFocus (windowfocus; what openbox and fluxbox honour on a headless display).
focus_on() {
  xdotool windowactivate "$1" 2>/dev/null || true
  xdotool windowfocus --sync "$1"
}
focused_class() { "$binary" focus windows 2>/dev/null | sed -n 's/^\* \([^\t]*\).*/\1/p'; }

# Start from the plain xterm, so the first switch is the Eyes one, not the watcher's own start.
focus_on "$term"
for _ in $(seq 30); do
  [ "$(focused_class)" = XTerm ] && break
  sleep 0.1
done
[ "$(focused_class)" = XTerm ] || fail "could not focus the plain xterm first: $(show "$out/windows.txt")"

# The watcher follows focus: Eyes brings its profile, the plain xterm (no rule) the default.
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
focus_on "$eyes"
expect_active eyes "focusing Eyes"
focus_on "$term"
expect_active default "focusing the plain xterm"
focus_on "$eyes"
expect_active eyes "focusing Eyes again"
grep -q "^window 'Eyes'" "$out/watch.log" || fail "the watcher never named the Eyes window: $(show "$out/watch.log")"
echo "ok: $wm on $display; focus windows listed both; the watcher switched eyes, default, eyes; output in $out"
