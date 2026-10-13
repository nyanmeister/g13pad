#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Actual MPD boundary, confined to a private library/socket and null audio output.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-mpd-check 0.2.85'; exit 0; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
binary=${1:-"$repo/target/release/g13map"}
fixture=$(mktemp -d /tmp/g13pad-mpd.XXXXXX)
daemon_pid=''
cleanup() {
  [ -n "$daemon_pid" ] || return 0
  kill "$daemon_pid" 2>/dev/null || true
  for _try in $(seq 1 30); do kill -0 "$daemon_pid" 2>/dev/null || break; sleep .05; done
  if kill -0 "$daemon_pid" 2>/dev/null; then kill -KILL "$daemon_pid" 2>/dev/null || true; fi
  wait "$daemon_pid" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
mkdir -p "$fixture/music" "$fixture/runtime"
chmod 700 "$fixture/runtime"
export XDG_RUNTIME_DIR="$fixture/runtime" MPD_HOST="$fixture/socket" MPD_PORT=6600 PULSE_SERVER="unix:$fixture/no-pulse"
# 12 seconds of 8 kHz, mono, 16-bit silence, without media/tool dependencies.
perl -e 'my $n=12*8000*2; print "RIFF",pack("V",36+$n),"WAVEfmt ",pack("VvvVVvv",16,1,1,8000,16000,2,16),"data",pack("V",$n),chr(0)x$n' > "$fixture/music/test.wav"
cat > "$fixture/mpd.conf" <<CONFIG
music_directory "$fixture/music"
db_file "$fixture/database"
bind_to_address "$fixture/socket"
zeroconf_enabled "no"
audio_output {
  type "null"
  name "Private statistics test"
  sync "yes"
}
CONFIG
mpd --no-daemon --stderr "$fixture/mpd.conf" > "$fixture/mpd.log" 2>&1 &
daemon_pid=$!
for _try in $(seq 1 60); do [ ! -S "$fixture/socket" ] || break; kill -0 "$daemon_pid"; sleep .05; done
mpc --wait update > "$fixture/update.log"
"$binary" --version
"$binary" mpd status > "$fixture/stopped.txt"
rg -q 'SONGS 1' "$fixture/stopped.txt"
rg -q 'LIB 0M12S' "$fixture/stopped.txt"
rg -q 'DB AGE 0M' "$fixture/stopped.txt"
"$binary" mpd preview "$fixture/screen.lpbm"
[ "$(wc -c < "$fixture/screen.lpbm")" -eq 960 ]
mpc add test.wav
mpc play > "$fixture/play.log"
sleep 2.2
"$binary" mpd status > "$fixture/playing.txt"
rg -q 'PLAYED 0M[1-9][0-9]*S' "$fixture/playing.txt"
mpc pause > "$fixture/pause.log"
sleep .3
"$binary" mpd status > "$fixture/paused.txt"
sleep 1.1
"$binary" mpd status > "$fixture/paused-again.txt"
[ "$(rg '^PLAYED' "$fixture/paused.txt")" = "$(rg '^PLAYED' "$fixture/paused-again.txt")" ]
cleanup
daemon_pid=''
if "$binary" mpd status > "$fixture/offline.txt" 2>&1; then exit 1; fi
rg -q 'MPD DISCONNECTED' "$fixture/offline.txt"
printf 'Private native MPD library, playback counter, pause, preview and disconnect passed: %s\n' "$fixture"
