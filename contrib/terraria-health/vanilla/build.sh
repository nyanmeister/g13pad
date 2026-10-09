#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
if [ "${1:-}" = --version ]; then echo 'g13map-terraria-vanilla-build 0.1.1'; exit 0; fi
if [ "$#" -ne 2 ]; then echo 'usage: build.sh TERRARIA_DIR EMPTY_OUTPUT_DIR' >&2; exit 2; fi
game=$(realpath -- "$1")
out=$(realpath -m -- "$2")
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$out"
cecil=${G13MAP_CECIL_DLL:-}
if [ -z "$cecil" ]; then
    for candidate in /usr/lib/mono/gac/Mono.Cecil/0.11.*/* /usr/lib/mono/gac/Mono.Cecil/0.11.*/Mono.Cecil.dll; do
        case "$candidate" in */Mono.Cecil.dll) if [ -f "$candidate" ]; then cecil=$candidate; break; fi;; esac
    done
fi
if [ ! -f "$cecil" ]; then echo 'Mono.Cecil 0.11 is required (or set G13MAP_CECIL_DLL)' >&2; exit 1; fi
mcs -sdk:4 -target:library -out:"$out/G13TerrariaObserver.dll" "$here/Observer.cs"
mcs -sdk:4 -r:"$cecil" -out:"$out/Patcher.exe" "$here/Patcher.cs"
set -- "$game/Terraria.exe" "$out/Terraria.G13.exe" "$out/G13TerrariaObserver.dll" "$game"
# Wine Mono's XNA facades forward types to WineMono.FNA. Supply metadata only;
# these dependencies are neither copied nor changed in the game installation.
for root in /usr/share/wine/mono/*/lib/mono/gac /usr/share/steam/compatibilitytools.d/*/files/share/wine/mono/*/lib/mono/gac; do
    for directory in "$root"/Microsoft.Xna.Framework*/* "$root"/WineMono.FNA*/*; do
        if [ -d "$directory" ]; then set -- "$@" "$directory"; fi
    done
done
mono "$out/Patcher.exe" "$@"
if [ -f "$game/Terraria.bin.x86_64" ]; then
    cp -- "$game/Terraria.bin.x86_64" "$out/Terraria.G13.bin.x86_64"
fi
if [ -f "$game/Terraria.exe.config" ]; then
    cp -- "$game/Terraria.exe.config" "$out/Terraria.G13.exe.config"
fi
sha256sum "$game/Terraria.exe" | cut -d ' ' -f 1 > "$out/original.sha256"
