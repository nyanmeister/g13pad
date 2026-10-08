#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Runs g13map-watch.service from a warm cargo release build instead of the installed
# package, for trying watcher changes without a package cycle: a systemd drop-in
# overrides ExecStart. `dev-watch.sh` builds and switches; `dev-watch.sh off` removes
# the drop-in and restarts the service on the package's binary.
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
dropin="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/g13map-watch.service.d"
if [ "${1:-}" = off ]; then
  rm -f "$dropin/dev.conf"
  rmdir "$dropin" 2>/dev/null || true
  systemctl --user daemon-reload
  systemctl --user restart g13map-watch.service
  echo "g13map-watch.service: back on the installed g13map"
  exit 0
fi
target=${CARGO_TARGET_DIR:-$repo/target}
(cd "$repo" && cargo build --release -q --bin g13map)
bin="$target/release/g13map"
"$bin" --version
mkdir -p "$dropin"
printf '[Service]\nExecStart=\nExecStart=%s watch\n' "$bin" > "$dropin/dev.conf"
systemctl --user daemon-reload
systemctl --user restart g13map-watch.service
echo "g13map-watch.service: running $bin (dev-watch.sh off restores the package)"
