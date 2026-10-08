#!/bin/sh
# Usage: sh tools/build-driver-lcd-checks.sh ~/.local/src/g13d-audit
# SANITIZER=thread selects ThreadSanitizer for the lifecycle/report checks.
set -eu
src=$(realpath "$1")
case "$src" in /media/*|/mnt/*) echo 'Build in a local source checkout, not on a mounted share' >&2; exit 1;; esac
tools_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
sanitizer=${SANITIZER:-address,undefined}
mkdir -p "$src/build-audit"
cd "$src"
for check in thread-proof manager-proof timing; do
  # Timing is already built by the default invocation; keep sanitizer builds independent.
  if [ "$sanitizer" = thread ] && [ "$check" = timing ]; then continue; fi
  if [ "$check" = timing ]; then
    set -- -O2 -DG13_TIMING_THREADS
    target=timing-threaded
  else
    set -- -g -O1 -fno-omit-frame-pointer "-fsanitize=$sanitizer"
    target=$check
    [ "$sanitizer" != thread ] || target="$target-tsan"
  fi
  if [ "$check" = manager-proof ]; then
    set -- "$@" -Wl,--wrap=open,--wrap=access,--wrap=ioctl
  fi
  clang++ -std=c++17 -pthread "$@" -include algorithm \
    -I. -I/usr/include/libevdev-1.0 "$tools_dir/driver-$check.cpp" \
    g13_action.cpp g13_device.cpp g13_fonts.cpp g13_hotplug.cpp g13_keys.cpp \
    g13_lcd.cpp g13_log.cpp g13_manager.cpp g13_profile.cpp g13_stick.cpp helper.cpp \
    -lusb-1.0 -llog4cpp -levdev -Wl,--wrap=write -o "build-audit/$target"
done
