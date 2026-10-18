#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
case ${1:-} in
  --version) echo 'everspace-g13-check 0.1.0'; exit 0;;
  --help) echo 'Usage: check.sh NATIVE_GAME_DIR READER_BINARY G13MAP (all writes private)'; exit 0;;
esac
game=$(realpath "${1:?native game directory}")
reader=$(realpath "${2:?built g13map-everspace}")
cli=$(realpath "${3:?ship-capable g13map}")
scratch=$(mktemp -d /tmp/g13-everspace-check.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
private_home="$scratch/default home"
fake_game="$private_home/.local/share/Steam/steamapps/common/EVERSPACE"
mkdir -p "$fake_game/RSG/Binaries/Linux" "$private_home/.config/g13map" "$scratch/commands"
ln -s "$game/RSG/Binaries/Linux/RSG-Linux-Shipping" "$fake_game/RSG/Binaries/Linux/RSG-Linux-Shipping"
ln -s "$cli" "$scratch/commands/g13map"
ln -s "$reader" "$scratch/commands/g13map-everspace"
export HOME="$private_home" XDG_CONFIG_HOME="$private_home/.config" XDG_STATE_HOME="$private_home/.local/state"
export XDG_RUNTIME_DIR="$scratch/runtime" G13MAP_PIPE="$scratch/private pipe" G13MAP_OUT_PIPE="$scratch/private out"
export G13MAP_UNIT=0 G13MAP_ANALOG=1 G13MAP_ANALOG_MAP="$scratch/private analog.map"
unset G13MAP_CONFIG G13MAP_SHIP_FILE
export PATH="$scratch/commands:$PATH"
"$cli" profile create default
"$cli" profile bind default G1 KEY_A
"$cli" focus set unrelated default
"$here/install.sh" --binary "$reader"
"$private_home/.local/bin/g13map-everspace" --version
"$private_home/.local/bin/g13map-everspace-steam" --version
"$private_home/.local/bin/g13map-everspace-steam" --help > "$scratch/wrapper-help"
grep -Fq 'g13map-everspace' "$scratch/wrapper-help"
"$cli" show everspace > "$scratch/profile"
grep -Fxq '# ship on' "$scratch/profile"
grep -Fxq 'bind G1 KEY_A' "$scratch/profile"
"$cli" focus show > "$scratch/focus"
grep -Fq 'unrelated' "$scratch/focus"
grep -Fq 'RSG-Linux-Shipping' "$scratch/focus"
test ! -e "$G13MAP_PIPE"
# Execute the generated wrapper with literal metacharacters, spaces and an exit
# status that telemetry must preserve. No command string is constructed from args.
# shellcheck disable=SC2016 # Literal metacharacters are the input under test.
argument='spaces; $(touch must-not-exist) `false` *'
set +e
# shellcheck disable=SC2016 # Positional parameters belong to the child shell.
"$private_home/.local/bin/g13map-everspace-steam" /bin/sh -c 'printf "%s\n" "$1" > "$2"; exit 23' check "$argument" "$scratch/argument"
status=$?
set -e
test "$status" -eq 23
test "$(cat "$scratch/argument")" = "$argument"
test ! -e must-not-exist
custom_config="$scratch/custom config"
custom_bin="$scratch/custom bin"
custom_feed="$scratch/custom data/ship feed"
G13MAP_CONFIG="$custom_config" "$cli" profile create kept
G13MAP_CONFIG="$custom_config" "$cli" profile bind kept G2 KEY_Z
"$here/install.sh" --game-dir "$fake_game" --binary "$reader" --bin-dir "$custom_bin" --config-dir "$custom_config" --profile kept --feed "$custom_feed" --no-focus
before=$(sha256sum "$custom_config/profiles/kept.bind" | cut -d ' ' -f 1)
"$here/install.sh" --game-dir "$fake_game" --binary "$reader" --bin-dir "$custom_bin" --config-dir "$custom_config" --profile kept --feed "$custom_feed" --no-focus
test "$before" = "$(sha256sum "$custom_config/profiles/kept.bind" | cut -d ' ' -f 1)"
grep -Fxq 'bind G2 KEY_Z' "$custom_config/profiles/kept.bind"
test "$(cat "$custom_config/ship-feed")" = "$custom_feed"
test ! -e "$custom_config/focus"
"$custom_bin/g13map-everspace-steam" /bin/sleep 0.3
grep -Fxq 'wait ttl 3' "$custom_feed"
# Installed data directories have no checkout target/. Exercise PATH discovery
# using only the installer, exactly as a recipient package supplies it.
mkdir -p "$scratch/installed data"
cp "$here/install.sh" "$scratch/installed data/install.sh"
bash "$scratch/installed data/install.sh" --game-dir "$fake_game" --bin-dir "$scratch/installed bin" --no-profile
cmp "$reader" "$scratch/installed bin/g13map-everspace"
for invalid in '--missing-option' '--profile=bad'; do
  if "$here/install.sh" "$invalid" > "$scratch/reject" 2>&1; then echo 'FAIL: invalid option accepted'; exit 1; fi
done
if "$here/install.sh" --game-dir "$scratch/missing" --binary "$reader" --bin-dir "$scratch/absent" > "$scratch/reject" 2>&1; then echo 'FAIL: missing native game accepted'; exit 1; fi
test ! -e "$scratch/absent"
if "$here/install.sh" --game-dir "$fake_game" --binary "$reader" --bin-dir "$scratch/absent" --profile ../bad > "$scratch/reject" 2>&1; then echo 'FAIL: unsafe name accepted'; exit 1; fi
test ! -e "$scratch/absent"
echo 'PASS: default Steam discovery, custom install/config/feed/profile paths, base-profile copy, binding/rule preservation, repeated install, prerequisites, wrapper quoting and exit status'
