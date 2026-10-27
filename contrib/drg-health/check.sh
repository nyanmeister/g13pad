#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Checks install.sh against a fake game tree and runs the mod's feed-path resolution
# under plain Lua with the UE4SS globals stubbed. All writes are private. Needs lua.
set -euo pipefail
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
case ${1:-} in
  --version) echo 'drg-g13-check 0.1.0'; exit 0;;
  --help) echo 'Usage: check.sh (no arguments; all writes private)'; exit 0;;
esac
lua=$(command -v lua5.4 || command -v lua || command -v luajit) || { echo 'lua is required' >&2; exit 1; }
scratch=$(mktemp -d /tmp/g13-drg-check.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
home="$scratch/default home"
win64="$home/.local/share/Steam/steamapps/common/Deep Rock Galactic/FSD/Binaries/Win64"
mkdir -p "$win64/ue4ss/Mods"
: > "$win64/FSD-Win64-Shipping.exe"
printf 'DRGQoL : 1\n' > "$win64/ue4ss/Mods/mods.txt"
export HOME="$home"
unset XDG_STATE_HOME G13MAP_HEALTH_FILE G13MAP_FEED

# Defaults: Steam discovery under HOME, feed under ~/.local/state, enabled.txt since no list names it.
"$here/install.sh" > "$scratch/out1"
mod="$win64/ue4ss/Mods/G13Health"
cmp "$here/G13Health/Scripts/main.lua" "$mod/Scripts/main.lua"
test "$(cat "$mod/feed.txt")" = "Z:${home//\//\\}\\.local\\state\\g13map\\health"
test -f "$mod/enabled.txt"
test "$(cat "$win64/ue4ss/Mods/mods.txt")" = 'DRGQoL : 1'
test -d "$home/.local/state/g13map"
test ! -e "$win64/ue4ss/G13Health.backups"
grep -Fq 'Enabled by:' "$scratch/out1"

# Repeat with a list naming the mod: enabled.txt from the previous copy is kept, a backup is made.
printf 'DRGQoL : 1\nG13Health : 1\n' > "$win64/ue4ss/Mods/mods.txt"
"$here/install.sh" > "$scratch/out2"
test -f "$mod/enabled.txt"
backups=("$win64/ue4ss/G13Health.backups"/*)
test "${#backups[@]}" -eq 1
cmp "$here/G13Health/Scripts/main.lua" "${backups[0]}/Scripts/main.lua"
grep -Fq 'mods.txt or mods.json (unchanged)' "$scratch/out2"
for leftover in "$win64/ue4ss/Mods"/G13Health.new.* "$win64/ue4ss/Mods"/G13Health.old.*; do
  [[ ! -e $leftover ]] || { echo "FAIL: staging left behind: $leftover"; exit 1; }
done

# Non-default: explicit mods dir, feed with a space, another drive letter; mods.json names it.
custom="$scratch/custom mods"
mkdir -p "$custom"
printf '[{"mod_name": "G13Health", "mod_enabled": true}]\n' > "$custom/mods.json"
"$here/install.sh" --mods-dir "$custom" --feed "$scratch/custom state/health feed" --drive Y: > "$scratch/out3"
test "$(cat "$custom/G13Health/feed.txt")" = "Y:${scratch//\//\\}\\custom state\\health feed"
test ! -e "$custom/G13Health/enabled.txt"
test -d "$scratch/custom state"

# Rejections leave nothing behind.
for args in '--missing-option' '--feed relative/path' '--drive ZZ' '--game-dir'; do
  # shellcheck disable=SC2086 # Word splitting is the input under test.
  if "$here/install.sh" $args --mods-dir "$scratch/absent" > "$scratch/reject" 2>&1; then echo "FAIL: accepted $args"; exit 1; fi
done
if "$here/install.sh" --mods-dir "$scratch/absent" > "$scratch/reject" 2>&1; then echo 'FAIL: missing Mods folder accepted'; exit 1; fi
test ! -e "$scratch/absent"
if HOME="$scratch/nowhere" "$here/install.sh" > "$scratch/reject" 2>&1; then echo 'FAIL: missing game accepted'; exit 1; fi

# The mod under plain Lua: stub the UE4SS API, load main.lua as installed, read the
# "loaded; feed ..." line, then run one poll tick without a pawn and read the feed.
cat > "$scratch/harness.lua" <<'EOF'
local script, expectFeed, expectSource, probeWrite = arg[1], arg[2], arg[3], arg[4]
local lines, loops = {}, {}
local realPrint = print
print = function(s) lines[#lines + 1] = s end
NotifyOnNewObject = function() end
ExecuteInGameThread = function(f) f() end
FindAllOf = function() return nil end
FindFirstOf = function() return nil end
LoopAsync = function(_, f) loops[#loops + 1] = f end
dofile(script)
local loaded
for _, l in ipairs(lines) do
	loaded = l:match("^%[G13Health%] loaded; feed (.-), poll") or loaded
end
local want = string.format("%s (%s)", expectFeed, expectSource)
if loaded ~= want then
	realPrint(string.format("FAIL: feed resolved to %q, expected %q", tostring(loaded), want))
	os.exit(1)
end
if probeWrite == "write" then
	assert(#loops == 1, "one poll loop")
	loops[1]()
	local f = assert(io.open(expectFeed, "r"))
	local got = f:read("l")
	f:close()
	if got ~= "wait ttl 15" then
		realPrint(string.format("FAIL: feed line %q", tostring(got)))
		os.exit(1)
	end
end
EOF
# Installed copy: feed.txt wins over HOME. A host path in feed.txt so the write can land.
printf '%s\n' "$scratch/host feed" > "$mod/feed.txt"
"$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" "$scratch/host feed" feed.txt write
# A trailing CRLF in feed.txt is trimmed.
printf '%s\r\n' "$scratch/host feed" > "$mod/feed.txt"
"$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" "$scratch/host feed" feed.txt
# The environment variable wins over feed.txt.
G13MAP_FEED="$scratch/env feed" "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" "$scratch/env feed" G13MAP_FEED
# Without feed.txt: HOME through Z: (old Wine); Wine's own host-home variables, Unix or NT
# form (current Wine, which hides HOME); and nowhere at all without any of them.
rm "$mod/feed.txt"
"$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" "Z:${home//\//\\}\\.local\\state\\g13map\\health" HOME
env -u HOME -u XDG_STATE_HOME WINE_HOST_HOME='/srv/other home' WINEHOMEDIR='\??\Z:\srv\other home' "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" 'Z:\srv\other home\.local\state\g13map\health' WINE_HOST_HOME
env -u HOME -u XDG_STATE_HOME WINEHOMEDIR='\??\Z:\srv\nt home' "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" 'Z:\srv\nt home\.local\state\g13map\health' WINEHOMEDIR
env -u HOME -u XDG_STATE_HOME WINEHOMEDIR='\??\unix\srv\unix home' "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" 'Z:\srv\unix home\.local\state\g13map\health' WINEHOMEDIR
env -u HOME -u XDG_STATE_HOME WINEHOMEDIR='C:\users\steamuser' "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" nil 'no feed.txt beside the mod and no host home in the environment: run contrib/drg-health/install.sh, or set config.feed'
env -u HOME -u XDG_STATE_HOME "$lua" "$scratch/harness.lua" "$mod/Scripts/main.lua" nil 'no feed.txt beside the mod and no host home in the environment: run contrib/drg-health/install.sh, or set config.feed'
echo 'PASS: default Steam discovery, feed.txt and enabled.txt, backup and list preservation, custom mods/feed/drive, rejections, Lua resolution order (G13MAP_FEED > feed.txt > HOME > WINE_HOST_HOME > WINEHOMEDIR > none) and one feed write'
