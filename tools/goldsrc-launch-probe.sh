#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Temporary Steam launch diagnostic; no full environment dump.
set -eu
if [ "${1:-}" = --version ]; then
    printf '%s\n' 'g13map-goldsrc-probe 0.2.45'
    exit 0
fi
if [ "$#" -eq 0 ]; then
    echo 'usage: g13map-goldsrc-probe COMMAND [ARGS...]' >&2
    exit 2
fi
umask 077
logdir=${XDG_STATE_HOME:-"$HOME/.local/state"}/g13map/goldsrc-launch
mkdir -p -- "$logdir"
log=$(mktemp "$logdir/launch.XXXXXXXX.log")
exec >"$log" 2>&1
date --iso-8601=seconds
printf 'cwd: %s\n' "$PWD"
idx=0
for arg do
    printf 'argv[%s]: <%s>\n' "$idx" "$arg"
    idx=$((idx + 1))
done
printf 'LD_PRELOAD: <%s>\nLD_LIBRARY_PATH: <%s>\n' "${LD_PRELOAD-}" "${LD_LIBRARY_PATH-}"
printf 'HOME: <%s>\nXDG_STATE_HOME: <%s>\nSteamAppId: <%s>\nSteamGameId: <%s>\n' "$HOME" "${XDG_STATE_HOME-}" "${SteamAppId-}" "${SteamGameId-}"
export G13MAP_GOLDSRC_DEBUG=1
exec "${G13MAP_GOLDSRC_LAUNCHER:-"$HOME/.local/bin/g13map-goldsrc"}" "$@"
