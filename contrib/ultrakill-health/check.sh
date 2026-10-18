#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
case ${1:-} in
  --version) echo 'ultrakill-g13-check 0.2.0'; exit 0 ;;
  --help) echo 'Usage: check.sh GAME_DIR [G13MAP] (installed BepInEx core; all writes private)'; exit 0 ;;
esac
game=$(realpath "${1:?ULTRAKILL game directory required}")
scratch=$(mktemp -d /tmp/g13-ultrakill-check.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
core="$game/BepInEx/core"
csc -nologo -optimize+ -out:"$scratch/accuracy.exe" "$here/Accuracy.cs" "$here/tests/AccuracyChecks.cs"
mono "$scratch/accuracy.exe"
csc -nologo -optimize+ -r:"$core/0Harmony.dll" -out:"$scratch/hooks.exe" "$here/Accuracy.cs" "$here/CombatTracking.cs" "$here/tests/HookChecks.cs"
MONO_PATH="$core" mono "$scratch/hooks.exe"
fake_home="$scratch/default home"
fake_game="$fake_home/.local/share/Steam/steamapps/common/ULTRAKILL"
mkdir -p "$fake_game/ULTRAKILL_Data" "$fake_game/BepInEx"
ln -s "$game/ULTRAKILL_Data/Managed" "$fake_game/ULTRAKILL_Data/Managed"
ln -s "$core" "$fake_game/BepInEx/core"
HOME="$fake_home" "$here/install.sh"
test -s "$fake_game/BepInEx/plugins/G13Health.dll"
grep -Fxq 'Enabled = true' "$fake_game/BepInEx/config/g13pad.health.cfg"
mkdir -p "$scratch/custom plugins" "$scratch/custom config"
custom_config="$scratch/custom config/settings.cfg"
printf '[Feed]\nPath = Z:\\existing\\health\n\n[Other]\nKeep = unchanged\n\n[Stats]\nEnabled = false\nPath = Z:\\old\\combat\n' > "$custom_config"
"$here/install.sh" --game-dir "$fake_game" --plugins-dir "$scratch/custom plugins" --config "$custom_config" --stats-feed 'Z:\custom path\combat'
grep -Fxq 'Path = Z:\existing\health' "$custom_config"
grep -Fxq 'Path = Z:\custom path\combat' "$custom_config"
grep -Fxq 'Keep = unchanged' "$custom_config"
grep -Fxq 'Enabled = true' "$custom_config"
before=$(sha256sum "$custom_config" | cut -d ' ' -f 1)
"$here/install.sh" --game-dir "$fake_game" --plugins-dir "$scratch/custom plugins" --config "$custom_config"
test "$before" = "$(sha256sum "$custom_config" | cut -d ' ' -f 1)"
test -n "$(ls -A "$scratch/custom plugins.backups")"
if "$here/install.sh" --game-dir "$scratch/missing" --plugins-dir "$scratch/must stay absent" > "$scratch/rejected.log" 2>&1; then
  echo 'FAIL: missing game accepted' >&2; exit 1
fi
test ! -e "$scratch/must stay absent"
if [[ -n ${2:-} ]]; then
  cli=$(realpath "$2")
  export G13MAP_CONFIG="$scratch/profile config" XDG_RUNTIME_DIR="$scratch/runtime"
  export XDG_STATE_HOME="$scratch/state" G13MAP_PIPE="$scratch/private pipe"
  export G13MAP_OUT_PIPE="$scratch/private out" G13MAP_UNIT=0 G13MAP_ANALOG=1
  export G13MAP_ANALOG_MAP="$scratch/private analog.map"
  "$cli" profile create integration-check
  "$cli" profile bind integration-check G1 KEY_A
  "$here/install.sh" --game-dir "$fake_game" --profile integration-check --g13map "$cli"
  "$cli" show integration-check > "$scratch/profile.txt"
  grep -Fxq '# combat on' "$scratch/profile.txt"
  grep -Fxq 'bind G1 KEY_A' "$scratch/profile.txt"
  test ! -e "$G13MAP_PIPE"
  echo 'PASS: real CLI profile selection preserves bindings and sends no hardware command'
fi
shopt -s globstar nullglob dotglob
plugin_candidates=("$fake_game"/BepInEx/plugins/**/*.dll)
test "${#plugin_candidates[@]}" -eq 1
echo 'PASS: recursive plugin scan sees only the installed DLL, never backup versions'
echo 'PASS: default Steam discovery, paths with spaces, custom feed/config/install paths, preservation, repeat install, backups, missing prerequisites'
