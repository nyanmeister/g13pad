#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
if [ "${1:-}" = --version ]; then echo 'g13pad-terraria-handler-build 0.1.0'; exit 0; fi
if [ "$#" -ne 2 ]; then
    echo 'usage: build.sh TMODLOADER_DIR OUTPUT_SAVE_DIR' >&2
    exit 2
fi
game=$(realpath -- "$1")
out=$(realpath -m -- "$2")
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# A separate save directory keeps the builder away from live enabled.json/saves.
# tModLoader includes Roslyn and .NET reference assemblies; no SDK is needed.
cd -- "$game"
exec "$game/dotnet/dotnet" "$game/tModLoader.dll" -server -nosteam \
    -tmlsavedirectory "$out" -build "$here/G13TerrariaHealth"
