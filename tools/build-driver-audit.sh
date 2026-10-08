#!/bin/sh
# Usage: sh tools/build-driver-audit.sh ~/.local/src/g13d-audit
# Source must be a local checkout; binaries stay in its build-audit directory.
set -eu
src=$(realpath "$1")
case "$src" in /media/*|/mnt/*) echo 'Build in a local source checkout, not on a mounted share' >&2; exit 1;; esac
tools_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$src/build-audit"
cd "$src"
for mode in proof fuzz state; do
  if [ "$mode" = proof ]; then
    set -- -DG13_AUDIT_PROOF -fsanitize=address,undefined
  elif [ "$mode" = state ]; then
    set -- -DG13_AUDIT_STATE -fsanitize=fuzzer,address,undefined
  else
    set -- -fsanitize=fuzzer,address,undefined
  fi
  clang++ -std=c++17 -pthread -g -O1 -fno-omit-frame-pointer "$@" \
    -include algorithm -I. -I/usr/include/libevdev-1.0 \
    "$tools_dir/driver-audit.cpp" g13_action.cpp g13_device.cpp g13_fonts.cpp \
    g13_hotplug.cpp g13_keys.cpp g13_lcd.cpp g13_log.cpp g13_manager.cpp \
    g13_profile.cpp g13_stick.cpp helper.cpp -lusb-1.0 -llog4cpp -levdev \
    -Wl,--wrap=write -o "build-audit/$mode"
done
