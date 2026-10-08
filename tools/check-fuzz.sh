#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Separate development binaries. Mock devices only; no USB, GUI or desktop input.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-fuzz-check 0.2.16'; exit 0; fi
if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  printf '%s\n' 'usage: check-fuzz.sh FUZZ_BUILD OUTPUT_DIRECTORY [RUNS_PER_TARGET]' >&2; exit 2
fi
build=$(realpath "$1")
mkdir -p "$2"
out=$(realpath "$2")
runs=${3:-100000}
case "$runs" in ''|*[!0-9]*|0) printf '%s\n' 'runs must be a positive integer' >&2; exit 2;; esac
export ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1
mkdir -p "$out/corpus/command" "$out/corpus/state" "$out/corpus/pbm" "$out/artifacts"
printf 'bind G10 KEY_LEFTCTRL\nbind G10 KEY_RESERVED\n' > "$out/corpus/command/bind"
printf 'stickzone add FUZZ\nbind STICK_UP !stickzone del STICK_UP\n' > "$out/corpus/command/zone-self-delete"
printf '\000\042\000\001\027\005\062\011\005\022' > "$out/corpus/state/rebind-zone"
{ printf 'P4\n160 43\n'; dd if=/dev/zero bs=860 count=1 status=none; } > "$out/corpus/pbm/image"
printf 'P1\n2 2\n0 1\n1 0\n' > "$out/corpus/pbm/ascii"
for target in command state pbm; do
  max_len=4096
  [ "$target" != pbm ] || max_len=16384
  "$build/$target-fuzz" "$out/corpus/$target" "-runs=$runs" "-max_len=$max_len" \
    -seed=20261002 -detect_leaks=1 "-artifact_prefix=$out/artifacts/" > "$out/$target.log" 2>&1
  printf '%s\n' "$target: completed (see $out/$target.log)"
done
