#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Real libmpdclient/SQLite/service boundary; no desktop audio, files or queue touched.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-music-check 0.2.85'; exit 0; fi
binary=${1:?path to g13music}
fixture=$(mktemp -d /tmp/g13pad-music.XXXXXX)
mpd_pid=''
music_pid=''
watcher_pid=''
reader_pid=''
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cleanup() {
  for pid in "$watcher_pid" "$reader_pid" "$music_pid" "$mpd_pid"; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
    for _try in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep .05; done
    if kill -0 "$pid" 2>/dev/null; then kill -KILL "$pid" 2>/dev/null || true; fi
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup EXIT HUP INT TERM
mkdir -m 700 "$fixture/runtime" "$fixture/state"
mkdir -p "$fixture/music/A" "$fixture/music/B" "$fixture/music/C"
export XDG_RUNTIME_DIR="$fixture/runtime" XDG_STATE_HOME="$fixture/state" MPD_HOST="$fixture/socket" MPD_PORT=6600 G13MUSIC_PLAY_SECONDS=2
perl -e 'my $n=12*8000*2; print "RIFF",pack("V",36+$n),"WAVEfmt ",pack("VvvVVvv",16,1,1,8000,16000,2,16),"data",pack("V",$n),chr(0)x$n' > "$fixture/music/A/1.wav"
for path in A/2 B/1 B/2 C/1 C/2; do cp "$fixture/music/A/1.wav" "$fixture/music/$path.wav"; done
cat > "$fixture/mpd.conf" <<CONFIG
music_directory "$fixture/music"
db_file "$fixture/database"
sticker_file "$fixture/stickers.sqlite"
bind_to_address "$fixture/socket"
zeroconf_enabled "no"
audio_output {
 type "null"
 name "Private history test"
 sync "yes"
}
CONFIG
mpd --no-daemon --stderr "$fixture/mpd.conf" > "$fixture/mpd.log" 2>&1 &
mpd_pid=$!
for _try in $(seq 1 60); do [ ! -S "$fixture/socket" ] || break; kill -0 "$mpd_pid"; sleep .05; done
mpc --wait update > "$fixture/update.log"
for path in A/1 B/2 A/2 C/2 B/1 C/1; do mpc add "$path.wav"; done
mpc sticker A/1.wav set rating 6
mpc sticker A/1.wav set playcount 12
"$binary" --version
"$binary" run > "$fixture/music.log" 2>&1 &
music_pid=$!
for _try in $(seq 1 60); do [ ! -S "$fixture/runtime/g13music/control.sock" ] || break; kill -0 "$music_pid"; sleep .05; done
"$binary" status > "$fixture/initial.txt"
mpc play > "$fixture/play.log"
sleep 4.2
mpc pause > "$fixture/pause.log"
sleep 1.2
"$binary" bookmark
"$binary" track > "$fixture/track.json"
perl -MJSON::PP -e 'local $/; open(my $f,"<",$ARGV[0]) or die $!; my $j=decode_json(<$f>); die "wrong track snapshot" unless $j->{uri} eq "A/1.wav" && $j->{endpoint} eq $ARGV[1] && $j->{playcount} == 13 && $j->{lastplayed} > 0' "$fixture/track.json" "unix:$fixture/socket"
db="$fixture/state/g13music/history.sqlite"
[ "$(sqlite3 -readonly "$db" "SELECT playcount FROM tracks WHERE uri='A/1.wav'")" = 13 ]
heard=$(sqlite3 -readonly "$db" "SELECT listened_ms FROM tracks WHERE uri='A/1.wav'")
[ "$heard" -ge 2000 ]
[ "$heard" -le 4200 ]
sleep 1.2
"$binary" favourite
[ "$(sqlite3 -readonly "$db" "SELECT listened_ms FROM tracks WHERE uri='A/1.wav'")" = "$heard" ]
"$binary" rate-up
[ "$(mpc sticker A/1.wav get rating)" = 'rating=8' ]
"$binary" rate-down
[ "$(mpc sticker A/1.wav get rating)" = 'rating=6' ]
kill "$music_pid"
wait "$music_pid"
music_pid=''
"$binary" run >> "$fixture/music.log" 2>&1 &
music_pid=$!
for _try in $(seq 1 60); do [ ! -S "$fixture/runtime/g13music/control.sock" ] || break; sleep .05; done
"$binary" status
[ "$(sqlite3 -readonly "$db" 'SELECT count(*) FROM sessions')" = 1 ]
if "$binary" run > "$fixture/duplicate.txt" 2>&1; then exit 1; fi
rg -q 'already running' "$fixture/duplicate.txt"
mpc seek 9 > "$fixture/seek.log"
sleep 1.2
"$binary" bookmark
[ "$(sqlite3 -readonly "$db" "SELECT playcount FROM tracks WHERE uri='A/1.wav'")" = 13 ]
[ "$(sqlite3 -readonly "$db" "SELECT listened_ms FROM tracks WHERE uri='A/1.wav'")" = "$heard" ]
mpc random on > "$fixture/random.log"
"$binary" shuffle-albums
mpc -f '%file%' playlist > "$fixture/albums.txt"
[ "$(sed -n '1p' "$fixture/albums.txt")" = A/1.wav ]
[ "$(sed -n '2p' "$fixture/albums.txt")" = A/2.wav ]
[ "$(wc -l < "$fixture/albums.txt")" -eq 6 ]
mpc status | rg -q 'random: off'
"$binary" smart-shuffle
"$binary" next-album
"$binary" previous-album
[ "$(mpc -f '%file%' current)" = A/1.wav ]
"$binary" stop
if [ -n "${2:-}" ]; then
  map=$2
  export G13MAP_CONFIG="$fixture/config" G13MAP_PIPE="$fixture/input" G13MAP_OUT_PIPE="$fixture/output"
  export G13MAP_UNIT=0 G13MAP_ANALOG=0 G13MAP_ANALOG_MAP="$fixture/analog.map" I3SOCK="$fixture/no-i3"
  export G13MAP_DAEMON_CONFIG="$repo/packaging/default.bind"
  unset DISPLAY WAYLAND_DISPLAY
  mkdir -p "$G13MAP_CONFIG"
  mkfifo "$G13MAP_PIPE" "$G13MAP_OUT_PIPE"
  perl "$repo/tools/fakedaemon.pl" "$G13MAP_PIPE" "$fixture/frames" > "$fixture/reader.log" 2>&1 &
  reader_pid=$!
  "$map" profile create music
  "$map" profile bind music G1 '>MPD:rate-up;'
  "$map" use music
  "$map" watch > "$fixture/watcher.log" 2>&1 &
  watcher_pid=$!
  sleep .5
  printf 'MPD:rate-up;' > "$G13MAP_OUT_PIPE"
  for _try in $(seq 1 80); do
    [ "$(mpc sticker A/1.wav get rating)" != 'rating=8' ] || break
    sleep .05
  done
  [ "$(mpc sticker A/1.wav get rating)" = 'rating=8' ]
  "$map" music status > "$fixture/map-status.txt"
  rg -q 'MPD connected' "$fixture/map-status.txt"
  kill "$watcher_pid" "$reader_pid"
  wait "$watcher_pid" 2>/dev/null || true
  wait "$reader_pid" 2>/dev/null || true
  watcher_pid=''
  reader_pid=''
fi

"$binary" history 10 > "$fixture/history.txt"
rg -q 'A/1.wav' "$fixture/history.txt"
kill "$mpd_pid"
wait "$mpd_pid"
mpd_pid=''
sleep 2.2
"$binary" status | rg -q 'MPD disconnected'
"$binary" history 10 > "$fixture/offline-history.txt"
sqlite3 -readonly "$db" 'PRAGMA integrity_check' | rg -q '^ok$'
kill "$music_pid"
wait "$music_pid"
music_pid=''
printf 'Private playback accounting, Cantata-scale ratings, bookmark/favourite, restart, singleton, queue permutations and offline history passed: %s\n' "$fixture"
