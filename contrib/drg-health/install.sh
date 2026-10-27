#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Installs the G13Health UE4SS mod into Deep Rock Galactic and writes feed.txt beside
# it: the host feed path as the game sees it through Proton's Z: drive. The mod cannot
# derive that path itself since GE-Proton 11-9 (Wine drops HOME and XDG_* from the
# Windows environment), so the installer resolves it once, here, on the host.
set -euo pipefail
version=0.1.0
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
game='' mods='' drive=Z:
feed=${G13MAP_HEALTH_FILE:-${XDG_STATE_HOME:-$HOME/.local/state}/g13map/health}
usage() {
  cat <<'EOF'
Usage: install.sh [OPTIONS]
  --game-dir DIR   Deep Rock Galactic folder (default: Steam libraries under HOME)
  --mods-dir DIR   UE4SS Mods folder (default: GAME/FSD/Binaries/Win64/ue4ss/Mods)
  --feed FILE      host path of the health feed (default: XDG_STATE_HOME/g13map/health)
  --drive LETTER:  Wine drive that maps the host root (default: Z:)
  --version        print installer version
  --help           show this help
Requires UE4SS already installed in the game (its Mods folder must exist). Copies the
G13Health mod there, keeps a backup of a previous copy, writes feed.txt with the
game-visible feed path and, when neither mods.txt nor mods.json names the mod, enables
it with an enabled.txt in its folder. The change loads on the next game launch; close
the game first. No downloads, Steam settings, services, privileges or packages.
EOF
}
while (($#)); do
  case $1 in
    --version) echo "drg-g13-install $version"; exit 0;;
    --help) usage; exit 0;;
    --game-dir|--mods-dir|--feed|--drive)
      (($# >= 2)) || { echo "Missing value for $1" >&2; exit 2; }
      case $1 in
        --game-dir) game=$2;; --mods-dir) mods=$2;; --feed) feed=$2;; --drive) drive=$2;;
      esac
      shift 2;;
    *) echo "Unknown option: $1" >&2; exit 2;;
  esac
done
[[ -n $feed && $feed == /* && $feed != *$'\n'* && $feed != *$'\r'* ]] || { echo 'The feed must be one absolute host path' >&2; exit 2; }
[[ $drive =~ ^[A-Za-z]:$ ]] || { echo 'The drive is a letter and a colon, like Z:' >&2; exit 2; }
if [[ -z $mods ]]; then
  if [[ -z $game ]]; then
    for candidate in "$HOME/.local/share/Steam/steamapps/common/Deep Rock Galactic" "$HOME/.steam/steam/steamapps/common/Deep Rock Galactic"; do
      if [[ -f $candidate/FSD/Binaries/Win64/FSD-Win64-Shipping.exe ]]; then game=$candidate; break; fi
    done
  fi
  [[ -n $game && -f $game/FSD/Binaries/Win64/FSD-Win64-Shipping.exe ]] || { echo 'Game missing; use --game-dir' >&2; exit 1; }
  mods=$game/FSD/Binaries/Win64/ue4ss/Mods
fi
[[ -d $mods ]] || { echo "UE4SS Mods folder missing ($mods); install UE4SS first or use --mods-dir" >&2; exit 1; }
[[ -f $here/G13Health/Scripts/main.lua ]] || { echo "Mod source missing beside this script ($here/G13Health)" >&2; exit 1; }
mods=$(realpath -- "$mods")
feed=$(realpath -m -- "$feed")
target=$mods/G13Health
if [[ -d $target ]]; then
  # Outside Mods: UE4SS treats every folder in there as a mod candidate.
  backup="$(dirname -- "$mods")/G13Health.backups/$(date -u +%Y%m%dT%H%M%SZ)-$$"
  mkdir -p -- "$backup"
  cp -a -- "$target/." "$backup/"
  echo "Backup: $backup"
fi
staging=$(mktemp -d -- "$mods/G13Health.new.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT
cp -a -- "$here/G13Health/." "$staging/"
# The path as Wine presents the host root: DRIVE plus the host path with backslashes.
printf '%s%s\n' "$drive" "${feed//\//\\}" > "$staging/feed.txt"
if [[ -d $target ]]; then
  [[ ! -f $target/enabled.txt ]] || cp -p -- "$target/enabled.txt" "$staging/enabled.txt"
fi
listed=false
if [[ -f $mods/mods.txt ]] && grep -Eq '^[[:space:]]*G13Health[[:space:]]*:' "$mods/mods.txt"; then listed=true; fi
if [[ -f $mods/mods.json ]] && grep -Fq '"G13Health"' "$mods/mods.json"; then listed=true; fi
if ! $listed && [[ ! -f $staging/enabled.txt ]]; then : > "$staging/enabled.txt"; fi
if [[ -d $target ]]; then
  old=$(mktemp -d -- "$mods/G13Health.old.XXXXXX")
  mv -- "$target" "$old/G13Health"
  mv -- "$staging" "$target" || { mv -- "$old/G13Health" "$target"; rmdir -- "$old"; exit 1; }
  rm -rf -- "$old"
else
  mv -- "$staging" "$target"
fi
trap - EXIT
mkdir -p -- "$(dirname -- "$feed")"
echo "Installed: $target"
echo "Feed (game side): $(cat "$target/feed.txt")"
if $listed; then echo "Enabled by: $mods/mods.txt or mods.json (unchanged)"; else echo "Enabled by: $target/enabled.txt"; fi
echo 'Loads on the next game launch; UE4SS.log then shows "[G13Health] loaded; feed Z:... (feed.txt)".'
