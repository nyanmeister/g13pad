#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Exercise the real mpv Lua/property boundary without a display or sound output.
set -eu
if [ "${1:-}" = --version ]; then
    printf 'g13-mpv-check 0.2.76\n'
    exit 0
fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
probe=$(mktemp -d /tmp/g13-mpv-check.XXXXXX)
mpv_pid=
cleanup() {
  if [ -n "$mpv_pid" ]; then
    kill "$mpv_pid" 2>/dev/null || true
    for _attempt in $(seq 1 20); do kill -0 "$mpv_pid" 2>/dev/null || break; sleep .05; done
    if kill -0 "$mpv_pid" 2>/dev/null; then kill -KILL "$mpv_pid" 2>/dev/null || true; fi
    wait "$mpv_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT HUP INT TERM
mkdir -p "$probe/state/g13map/mpv" "$probe/runtime"
chmod 700 "$probe/runtime"
ffmpeg -v error -f lavfi -i testsrc2=size=160x90:rate=24 -f lavfi -i sine=frequency=440:sample_rate=48000 -t 20 -c:v libx264 -preset ultrafast -c:a aac "$probe/test.mkv"
env XDG_STATE_HOME="$probe/state" XDG_RUNTIME_DIR="$probe/runtime" mpv --no-config --vo=null --ao=null --pause --idle=yes --keep-open=yes --input-ipc-server="$probe/ipc" --script="$repo/contrib/mpv/g13-lcd.lua" "$probe/test.mkv" > "$probe/mpv.log" 2>&1 &
mpv_pid=$!
feed="$probe/state/g13map/mpv/$mpv_pid.json"
for _attempt in $(seq 1 100); do
    if [ -s "$feed" ] && jq -e '.duration > 19 and .codec == "h264"' "$feed" >/dev/null; then break; fi
    sleep .05
done
jq -e '.version == 1 and .build == "0.2.76" and .paused == true and .duration > 19 and .codec == "h264"' "$feed" > /dev/null
ipc() { printf '%s\n' "$1" | socat -T 0.2 - "UNIX-CONNECT:$probe/ipc"; }
ipc '{"command":["seek",12,"absolute+exact"]}' > "$probe/seek.json"
sleep .5
jq -e '.paused == true and .position >= 11.9 and .position <= 12.1 and .decoder_drops == 0 and .display_drops == 0' "$feed" >/dev/null
cp "$feed" "$probe/paused.json"
ipc '{"command":["set_property","pause",false]}' > /dev/null
sleep .6
jq -e '.paused == false and .position > 12.2' "$feed" >/dev/null
load=$(jq -nc --arg path "$probe/test.mkv" '{command:["loadfile",$path]}')
ipc "$load" > /dev/null
sleep .5
jq -e '.position < 3 and .decoder_drops == 0 and .display_drops == 0' "$feed" >/dev/null
ipc '{"command":["stop"]}' > /dev/null
sleep .5
jq -e '.idle == true and .position == null and .duration == null' "$feed" >/dev/null
ipc '{"command":["quit"]}' > /dev/null
wait "$mpv_pid"
mpv_pid=
[ ! -e "$feed" ]
printf 'mpv playback, pause, seek, file switch, idle and shutdown passed: %s\n' "$probe"
