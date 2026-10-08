#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds G13Health.dll with Mono's csc against the game's own assemblies and BepInEx's
# core: build.sh GAME_DIR BEPINEX_CORE_DIR [OUT_DIR]
set -eu
game=${1:?game directory (the one with ULTRAKILL_Data)}
core=${2:?BepInEx/core directory}
out=${3:-.}
m="$game/ULTRAKILL_Data/Managed"
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
csc -nologo -target:library -nostdlib -noconfig -optimize+ -out:"$out/G13Health.dll" \
  -r:"$m/mscorlib.dll" -r:"$m/System.dll" -r:"$m/System.Core.dll" -r:"$m/netstandard.dll" \
  -r:"$m/UnityEngine.dll" -r:"$m/UnityEngine.CoreModule.dll" -r:"$m/Assembly-CSharp.dll" \
  -r:"$core/BepInEx.dll" -r:"$core/0Harmony.dll" \
  "$here/G13Health.cs"
echo "built $out/G13Health.dll"
