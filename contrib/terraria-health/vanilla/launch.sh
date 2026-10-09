#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Steam launch option: /path/to/g13map-terraria %command%
set -eu
if [ "${1:-}" = --version ]; then echo 'g13map-terraria 0.1.1'; exit 0; fi
if [ "$#" -eq 0 ]; then echo 'usage: g13map-terraria COMMAND [ARGS...]' >&2; exit 2; fi
# Rebuild argv without eval: Steam may wrap the game in several runtime commands.
remaining=$#
found=0
native=0
while [ "$remaining" -gt 0 ]; do
    argument=$1
    shift
    case "$argument" in
        */Terraria.exe|*/Terraria|*/Terraria.bin.x86_64)
            game=${argument%/*}
            original="$game/Terraria.exe"
            copy="$game/Terraria.G13.exe"
            manifest="$game/G13Terraria.original.sha256"
            if [ ! -f "$copy" ] || [ ! -f "$manifest" ] || [ ! -f "$game/G13TerrariaObserver.dll" ]; then
                echo 'Install the vanilla Terraria handler first.' >&2; exit 1
            fi
            current=$(sha256sum "$original" | cut -d ' ' -f 1)
            if [ "$current" != "$(cat "$manifest")" ]; then
                echo 'Terraria changed; rebuild the handler copy before launching.' >&2; exit 1
            fi
            case "$argument" in
                */Terraria.exe) argument=$copy;;
                *)
                    argument="$game/Terraria.G13.bin.x86_64"
                    if [ ! -x "$argument" ]; then echo 'Install the native MonoKickstart copy first.' >&2; exit 1; fi
                    native=1
                    ;;
            esac
            found=$((found + 1))
            ;;
    esac
    set -- "$@" "$argument"
    remaining=$((remaining - 1))
done
if [ "$found" -ne 1 ]; then echo 'Expected exactly one Terraria launcher in the Steam command.' >&2; exit 1; fi
if [ "$native" -eq 1 ]; then MONO_IOMAP=all; export MONO_IOMAP; fi
# Explicit custom variables survive Wine's filtering of HOME/XDG variables.
G13MAP_HEALTH_FILE=${G13MAP_HEALTH_FILE:-${XDG_STATE_HOME:-$HOME/.local/state}/g13map/health}
export G13MAP_HEALTH_FILE
exec "$@"
