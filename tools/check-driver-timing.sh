#!/bin/sh
# Synthetic USB + real driver/FIFO + real g13map animation scheduler.
# Usage: sh tools/check-driver-timing.sh TIMING_BINARY RESULT_DIRECTORY
set -eu
binary=$(realpath "$1")
results=$(realpath -m "$2")
mkdir -p "$results"
reader_pid=''
watcher_pid=''
cleanup() {
  [ -z "$watcher_pid" ] || kill "$watcher_pid" 2>/dev/null || true
  [ -z "$reader_pid" ] || kill "$reader_pid" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
for activity in idle busy; do
  case_dir="$results/$activity"
  mkdir -p "$case_dir/config/lcd" "$case_dir/config/profiles" "$case_dir/runtime"
  [ -p "$case_dir/pipe" ] || mkfifo "$case_dir/pipe"
  printf 'moving\n' > "$case_dir/config/active"
  printf '# lcd moving\n' > "$case_dir/config/profiles/moving.bind"
  perl -e 'for $i (0..24) { print pack("V",20), chr($i)x960 }' > "$case_dir/config/lcd/moving.anim"
  perl -e 'print chr(0)x960' > "$case_dir/config/lcd/moving.lpbm"
  "$binary" "$case_dir/pipe" "$case_dir/frames.txt" "$activity" 4 > "$case_dir/driver.log" 2>&1 &
  reader_pid=$!
  sleep 0.05
  G13MAP_CONFIG="$case_dir/config" G13MAP_PIPE="$case_dir/pipe" \
    XDG_RUNTIME_DIR="$case_dir/runtime" G13MAP_ANALOG=1 \
    "$HOME/.local/src/g13map/target/release/g13map" watch > "$case_dir/watcher.log" 2>&1 &
  watcher_pid=$!
  wait "$reader_pid"
  reader_pid=
  kill "$watcher_pid" 2>/dev/null || true
  wait "$watcher_pid" 2>/dev/null || true
  watcher_pid=
  awk -v mode="$activity" '
    NR==1 { first=$1 }
    NR>1 && $2<prev { if (wraps++) { cycles++; duration+=$1-lastwrap } lastwrap=$1 }
    { last=$1; prev=$2 }
    END {
      if (NR<2 || cycles<1) exit 1;
      printf "%s: %d frames, %.2f FPS, %.3f s mean loop (source 0.500 s)\n",mode,NR,(NR-1)/(last-first),duration/cycles
    }' "$case_dir/frames.txt"
done
