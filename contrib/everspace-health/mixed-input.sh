#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Optional native-game patch. Deliberately separate from the read-only observer.
set -euo pipefail
original=530388435db23422db9d6f2e42f663531525192faddeada5739245a849338f80
patched=b1dc728f77c5bb0f4bc3f0eed7124cdd20f595f8e55ef319a5973b6b6d8ccfed
game_exe="$HOME/.local/share/Steam/steamapps/common/EVERSPACE/RSG/Binaries/Linux/RSG-Linux-Shipping"
backup="${XDG_STATE_HOME:-$HOME/.local/state}/g13map/everspace-input-patch/original.bin"
action=check staged=''
usage() {
  cat <<'EOF'
Usage: mixed-input.sh [--check | --apply | --restore] [OPTIONS]
  --game-exe FILE   native EVERSPACE executable (default: standard Steam path)
  --backup FILE     original backup (default: XDG_STATE_HOME/g13map/everspace-input-patch/original.bin)
  --check           report original/patched/unsupported; never write (default)
  --apply           disable automatic gamepad mode; quit game first
  --restore         restore verified original backup; quit game first
  --version        print version
  --help           show help
Only native Linux Steam build 4246486 / game 1.3.5.36555 is supported.
This changes the game executable, not its saves or G13 telemetry observer.
EOF
}
die() { printf 'mixed-input: %s\n' "$*" >&2; exit 1; }
hash() { local sum; sum=$(sha256sum < "$1"); printf '%s' "${sum%% *}"; }
while (($#)); do
  case $1 in
    --help) usage; exit 0;;
    --version) echo 'everspace-mixed-input 0.1.0'; exit 0;;
    --check) action=check; shift;; --apply) action=apply; shift;; --restore) action=restore; shift;;
    --game-exe|--backup)
      (($# >= 2)) || die "Missing value for $1"
      case $1 in --game-exe) game_exe=$2;; --backup) backup=$2;; esac
      shift 2;;
    *) die "Unknown option: $1";;
  esac
done
for program in sha256sum realpath; do command -v "$program" >/dev/null || die "Missing $program"; done
[[ -f $game_exe ]] || die 'Native executable missing; use --game-exe'
game_exe=$(realpath -e -- "$game_exe")
backup=$(realpath -m -- "$backup")
[[ $game_exe != "$backup" ]] || die 'Backup must differ from game executable'
state() {
  current=$(hash "$game_exe")
  case $current in
    "$original") printf 'original\n';;
    "$patched") printf 'patched (automatic gamepad mode disabled)\n';;
    *) die 'Unsupported executable hash; no changes made. Inspect the new build first.';;
  esac
}
if [[ $action == check ]]; then state; exit 0; fi
for program in fuser flock cp mktemp dd mv; do command -v "$program" >/dev/null || die "Missing $program"; done
# Lock the game path, so callers using different backup paths still serialize.
exec 9>"$game_exe.g13-mixed-input.lock"
flock -n 9 || die 'Another mixed-input operation is running'
state
not_running() {
  local output
  # realpath made this absolute. psmisc fuser does not handle '-- FILE' here.
  if output=$(fuser -s "$game_exe" 2>&1); then
    die 'Game executable is in use; quit EVERSPACE first'
  elif [[ -n $output ]]; then
    die "Cannot check whether the game is running: $output"
  fi
}
not_running
if [[ $action == restore && $current == "$original" ]]; then echo 'Already original'; exit 0; fi
if [[ -e $backup ]]; then
  [[ -f $backup && $(hash "$backup") == "$original" ]] || die 'Backup is not the verified original; refusing to overwrite it'
elif [[ $action == apply && $current == "$original" ]]; then
  mkdir -p -- "$(dirname -- "$backup")"
  [[ ! -L $backup ]] || die 'Backup path is a symlink'
  cp --reflink=auto --preserve=mode,timestamps -- "$game_exe" "$backup"
  [[ $(hash "$backup") == "$original" ]] || die 'Backup verification failed'
else
  die 'Original backup missing; use --backup to select the retained original'
fi
if [[ $action == apply && $current == "$patched" ]]; then echo 'Already patched; original backup verified'; exit 0; fi
staged=$(mktemp -- "$game_exe.g13-mixed-input.XXXXXX")
trap '[[ -z $staged ]] || rm -f -- "$staged"' EXIT
if [[ $action == apply ]]; then
  cp --reflink=auto --preserve=mode,timestamps -- "$game_exe" "$staged"
  # FInputPreProcessor::EnableGamepadMode(): return immediately (x86-64 RET).
  printf '\303' | dd of="$staged" bs=1 seek=28828656 conv=notrunc status=none
  expected=$patched
else
  cp --reflink=auto --preserve=mode,timestamps -- "$backup" "$staged"
  expected=$original
fi
[[ $(hash "$staged") == "$expected" ]] || die 'Staged executable verification failed'
[[ $(hash "$game_exe") == "$current" ]] || die 'Game changed during operation; refusing to replace it'
not_running
mv -T -- "$staged" "$game_exe"
staged=''
[[ $(hash "$game_exe") == "$expected" ]] || die 'Installed executable verification failed'
printf 'Completed %s. Backup: %s\n' "$action" "$backup"
