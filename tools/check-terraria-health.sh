#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Start a copied tModLoader on its own display and with isolated saves/feed.
set -eu
if [ "${1:-}" = --version ]; then echo 'g13pad-terraria-check 0.1.0'; exit 0; fi
mode=${1:?native or proton}
root=$(realpath -- "${2:?test root containing game, save, and Windows runtime}")
display=${G13MAP_TEST_DISPLAY:-:97}
mkdir -p "$root/home" "$root/config" "$root/data" "$root/cache" "$root/state" "$root/run"
chmod 700 "$root/run"
Xvfb "$display" -screen 0 1024x768x24 -nolisten tcp > "$root/xvfb.log" 2>&1 &
xpid=$!
trap 'kill "$xpid" 2>/dev/null || true' EXIT
sleep 1
kill -0 "$xpid"
export DISPLAY="$display" HOME="$root/home" XDG_CONFIG_HOME="$root/config"
export XDG_DATA_HOME="$root/data" XDG_CACHE_HOME="$root/cache" XDG_STATE_HOME="$root/state"
export XDG_RUNTIME_DIR="$root/run" G13MAP_HEALTH_FILE="$root/state/health"
if [ "${G13MAP_TEST_DEFAULT_FEED:-0}" = 1 ]; then unset G13MAP_HEALTH_FILE; fi
export SDL_AUDIODRIVER=dummy LIBGL_ALWAYS_SOFTWARE=1 LANG=C.UTF-8 LC_ALL=C.UTF-8
unset I3SOCK SWAYSOCK LD_PRELOAD
rm -f "$root/state/health" "$root/state/g13map/health" "$root/home/.local/state/g13map/health"
printf 'font pango:monospace 8\nworkspace_layout default\n' > "$root/i3.conf"
i3 -c "$root/i3.conf" > "$root/i3.log" 2>&1 &
wm_pid=$!
trap 'kill "$wm_pid" "$xpid" 2>/dev/null || true' EXIT
cd "$root/game"
case "$mode" in
    native)
        set -- "$root/game/dotnet/dotnet" "$root/game/tModLoader.dll" \
            -nosteam -tmlsavedirectory "$root/save"
        ;;
    proton)
        : "${G13MAP_TEST_PROTON:?set the installed GE proton script}"
        : "${STEAM_COMPAT_CLIENT_INSTALL_PATH:?set the host Steam directory}"
        export STEAM_COMPAT_DATA_PATH="$root/proton-prefix"
        mkdir -p "$STEAM_COMPAT_DATA_PATH"
        export SteamAppId=1281930 SteamGameId=1281930
        export PROTON_LOG=1 PROTON_LOG_DIR="$root"
        set -- "$G13MAP_TEST_PROTON" run "$root/windows-dotnet/runtime/dotnet.exe" \
            "$root/game/tModLoader.dll" -nosteam -tmlsavedirectory "Z:$root/save"
        ;;
    *) echo 'mode must be native or proton' >&2; exit 2;;
esac
timeout -s TERM -k 5 "${G13MAP_TEST_SECONDS:-300}" "$@" > "$root/$mode-game.log" 2>&1 &
game_pid=$!
trap 'kill "$game_pid" "$wm_pid" "$xpid" 2>/dev/null || true' EXIT
while kill -0 "$game_pid" 2>/dev/null; do
    for feed in "$root/state/health" "$root/state/g13map/health" "$root/home/.local/state/g13map/health"; do
        if [ -f "$feed" ]; then
            printf '%s ' "$(date +%s.%N)"
            cat "$feed"
        fi
    done
    sleep 0.2
done > "$root/$mode-feed.log"
wait "$game_pid" || result=$?
exit "${result:-0}"
