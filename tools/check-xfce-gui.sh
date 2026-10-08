#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Native genmon rendering/clicks on private Xvfb + D-Bus + xfconf/FIFOs.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-xfce-check 0.2.16'; exit 0; fi
if [ "${1:-}" != --session ]; then
  [ "$#" -eq 1 ] || { printf '%s\n' 'usage: check-xfce-gui.sh OUTPUT_DIRECTORY' >&2; exit 2; }
  mkdir -p "$1"
  exec dbus-run-session -- sh "$0" --session "$(realpath "$1")"
fi
out=$2
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${G13PAD_TEST_BINARY:-/usr/bin/g13map}
fixture=$(mktemp -d /tmp/g13pad-xfce.XXXXXX)
display_pid='' wm_pid='' panel_pid='' reader_pid='' app_pid=''
cleanup() {
  for pid in "$app_pid" "$reader_pid" "$panel_pid" "$wm_pid" "$display_pid"; do
    [ -z "$pid" ] || { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
  done
  rm -rf "$fixture"
}
trap cleanup EXIT
Xvfb -displayfd 3 -screen 0 1200x1050x24 -nolisten tcp -noreset 3>"$fixture/display" >"$out/xvfb.log" 2>&1 &
display_pid=$!
i=0
while [ ! -s "$fixture/display" ]; do kill -0 "$display_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
DISPLAY=:$(cat "$fixture/display")
export DISPLAY LIBGL_ALWAYS_SOFTWARE=1 XDG_CONFIG_HOME="$fixture/xdg" XDG_CACHE_HOME="$fixture/cache"
export XDG_RUNTIME_DIR="$fixture/runtime" G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/pipe"
export G13MAP_OUT_PIPE="$fixture/out" G13MAP_ANALOG=0 G13MAP_UNIT=0
unset WAYLAND_DISPLAY SESSION_MANAGER
mkdir -p "$G13MAP_CONFIG/profiles" "$XDG_RUNTIME_DIR"
printf 'default\n' > "$G13MAP_CONFIG/active"
printf '# stick keys\nbind G1 KEY_D\n' > "$G13MAP_CONFIG/profiles/default.bind"
mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$fixture/reader.log" &
reader_pid=$!
i=0
while [ ! -f "$fixture/reader.log" ]; do kill -0 "$reader_pid"; i=$((i+1)); [ "$i" -lt 50 ]; sleep .1; done
xfwm4 > "$out/xfwm.log" 2>&1 &
wm_pid=$!
xfconf-query -c xfce4-panel -p /panels -n -a -t int -s 1
xfconf-query -c xfce4-panel -p /panels/panel-1/plugin-ids -n -a -t int -s 7
xfconf-query -c xfce4-panel -p /panels/panel-1/position -n -t string -s 'p=6;x=600;y=16'
xfconf-query -c xfce4-panel -p /panels/panel-1/length -n -t double -s 100
xfconf-query -c xfce4-panel -p /panels/panel-1/size -n -t int -s 32
xfconf-query -c xfce4-panel -p /panels/panel-1/position-locked -n -t bool -s true
xfconf-query -c xfce4-panel -p /plugins/plugin-7 -n -t string -s genmon
xfconf-query -c xfce4-panel -p /plugins/plugin-7/command -n -t string -s "$binary panel xfce"
xfconf-query -c xfce4-panel -p /plugins/plugin-7/use-label -n -t bool -s false
xfconf-query -c xfce4-panel -p /plugins/plugin-7/update-period -n -t int -s 1000
xfconf-query -c xfce4-panel -p /plugins/plugin-7/enable-single-row -n -t bool -s true
xfce4-panel > "$out/panel.log" 2>&1 &
panel_pid=$!
panel_win=$(timeout 15 xdotool search --sync --onlyvisible --class '^xfce4-panel$' | head -n 1)
sleep 1.2
xwd -id "$panel_win" -silent -out "$out/connected.xwd"
for x in 12 42; do
  xdotool mousemove --window "$panel_win" "$x" 16 click 1
  win=$(timeout 10 xdotool search --sync --class '^g13map$')
  app_pid=$(xdotool getwindowpid "$win")
  xdotool windowmap --sync "$win"
  sleep .4
  xwd -id "$win" -silent -out "$out/editor-$x.xwd"
  kill "$app_pid"
  app_pid=''
  sleep .3
done
kill "$reader_pid"
wait "$reader_pid" 2>/dev/null || true
reader_pid=''
xfce4-panel --plugin-event=genmon-7:refresh:bool:true
sleep 1.2
xwd -id "$panel_win" -silent -out "$out/disconnected.xwd"
printf '%s\n' 'PASS: installed XFCE icon/text clicks and connected/disconnected snapshots on private display'
