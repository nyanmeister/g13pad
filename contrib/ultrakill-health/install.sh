#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
version=0.2.0
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
game='' core='' plugins='' config='' stats_feed='' health_feed='' profile='' g13map='g13map'
usage() {
  cat <<'EOF'
Usage: install.sh [OPTIONS]
  --game-dir DIR       ULTRAKILL folder (default: standard Steam library under HOME)
  --core-dir DIR       BepInEx 5 core (default: GAME/BepInEx/core)
  --plugins-dir DIR    DLL destination (default: GAME/BepInEx/plugins)
  --config FILE        plugin config (default: GAME/BepInEx/config/g13pad.health.cfg)
  --stats-feed PATH    combat feed path as seen by the game; preserves saved path if omitted
  --health-feed PATH   health feed path as seen by the game; preserves saved path if omitted
  --profile NAME      optionally select the Combat display on an existing g13map profile
  --g13map PROGRAM     CLI used for --profile (default: g13map)
  --version           print installer version
  --help              show this help
Requires installed BepInEx 5 and Mono csc. No downloads or root access.
Close the game first. Backups are saved beside the installed DLL.
EOF
}
while (($#)); do
  case $1 in
    --version) echo "ultrakill-g13-install $version"; exit 0 ;;
    --help) usage; exit 0 ;;
    --game-dir|--core-dir|--plugins-dir|--config|--stats-feed|--health-feed|--profile|--g13map)
      (($# >= 2)) || { echo "Missing value for $1" >&2; exit 2; }
      case $1 in
        --game-dir) game=$2;; --core-dir) core=$2;; --plugins-dir) plugins=$2;;
        --config) config=$2;; --stats-feed) stats_feed=$2;; --health-feed) health_feed=$2;;
        --profile) profile=$2;; --g13map) g13map=$2;;
      esac
      shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
for value in "$stats_feed" "$health_feed"; do
  [[ $value != *$'\n'* && $value != *$'\r'* ]] || { echo 'Feed paths must be a single line' >&2; exit 2; }
done
if [[ -z $game ]]; then
  for candidate in "$HOME/.local/share/Steam/steamapps/common/ULTRAKILL" "$HOME/.steam/steam/steamapps/common/ULTRAKILL"; do
    if [[ -f $candidate/ULTRAKILL_Data/Managed/Assembly-CSharp.dll ]]; then game=$candidate; break; fi
  done
fi
[[ -n $game && -f $game/ULTRAKILL_Data/Managed/Assembly-CSharp.dll ]] || { echo 'Game assemblies missing; use --game-dir' >&2; exit 1; }
core=${core:-$game/BepInEx/core}
plugins=${plugins:-$game/BepInEx/plugins}
config=${config:-$game/BepInEx/config/g13pad.health.cfg}
[[ -f $core/BepInEx.dll && -f $core/0Harmony.dll ]] || { echo 'BepInEx 5 core missing; install the Windows x64 build for Proton first' >&2; exit 1; }
command -v csc >/dev/null || { echo 'Mono csc is required to compile against your game assemblies' >&2; exit 1; }
if [[ -n $profile ]]; then
  help=$("$g13map" --help)
  [[ $help == *'profile combat'* ]] || { echo 'Combat-capable g13map required for --profile' >&2; exit 1; }
fi
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
"$here/build.sh" "$game" "$core" "$scratch"
if [[ -f $config ]]; then cp -- "$config" "$scratch/config.old"; else : > "$scratch/config.old"; fi
# ENVIRON preserves literal Windows backslashes; awk -v would interpret escapes.
G13_INSTALL_STATS_PATH="$stats_feed" G13_INSTALL_HEALTH_PATH="$health_feed" awk '
  function finish() {
    if (section == "Stats") {
      if (!enabled) print "Enabled = true";
      if (!path && ENVIRON["G13_INSTALL_STATS_PATH"] != "") print "Path = " ENVIRON["G13_INSTALL_STATS_PATH"];
    }
    if (section == "Feed" && !path && ENVIRON["G13_INSTALL_HEALTH_PATH"] != "") print "Path = " ENVIRON["G13_INSTALL_HEALTH_PATH"];
  }
  { sub(/\r$/, "") }
  /^[[:space:]]*\[/ {
    finish(); section=$0; sub(/^[[:space:]]*\[/,"",section); sub(/\].*$/,"",section);
    enabled=0; path=0;
    if (section == "Stats") stats=1;
    if (section == "Feed") feed=1;
  }
  section == "Stats" && /^[[:space:]]*Enabled[[:space:]]*=/ { print "Enabled = true"; enabled=1; next }
  /^[[:space:]]*Path[[:space:]]*=/ {
    path=1;
    key=section == "Stats" ? "G13_INSTALL_STATS_PATH" : section == "Feed" ? "G13_INSTALL_HEALTH_PATH" : "";
    if (key != "" && ENVIRON[key] != "") { print "Path = " ENVIRON[key]; next }
  }
  { print }
  END {
    finish();
    if (!stats) print "\n[Stats]\nEnabled = true\nPath = " ENVIRON["G13_INSTALL_STATS_PATH"];
    if (!feed && ENVIRON["G13_INSTALL_HEALTH_PATH"] != "") print "\n[Feed]\nPath = " ENVIRON["G13_INSTALL_HEALTH_PATH"];
  }
' "$scratch/config.old" > "$scratch/config.new"
mkdir -p -- "$plugins" "$(dirname -- "$config")"
# BepInEx recursively scans plugins, including hidden directories. Keep DLL
# backups beside that directory so they cannot become candidate plugins.
backup="${plugins%/}.backups/$(date -u +%Y%m%dT%H%M%SZ)-$$"
if [[ -e $plugins/G13Health.dll || -e $config ]]; then
  mkdir -p -- "$backup"
  [[ ! -e $plugins/G13Health.dll ]] || cp -p -- "$plugins/G13Health.dll" "$backup/G13Health.dll"
  [[ ! -e $config ]] || cp -p -- "$config" "$backup/g13pad.health.cfg"
  printf 'DLL: %s\nConfig: %s\n' "$plugins/G13Health.dll" "$config" > "$backup/paths.txt"
  echo "Previous files: $backup"
fi
install -m 644 -- "$scratch/G13Health.dll" "$plugins/G13Health.dll.new"
mv -- "$plugins/G13Health.dll.new" "$plugins/G13Health.dll"
install -m 600 -- "$scratch/config.new" "$config.new"
mv -- "$config.new" "$config"
if [[ -n $profile ]]; then "$g13map" profile combat "$profile" on; fi
echo "Installed G13 Health + Combat $version in $plugins"
echo "Config: $config"
echo 'Proton launch option (retain your other options): WINEDLLOVERRIDES="winhttp=n,b" %command%'
echo 'Launch the game again to load the DLL. Combat is a separate display; the health integration remains available.'
