#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
if [ "${1:-}" = --version ]; then echo 'g13map-terraria-check 0.1.0'; exit 0; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cecil=${G13MAP_CECIL_DLL:-}
if [ -z "$cecil" ]; then
    for candidate in /usr/lib/mono/gac/Mono.Cecil/0.11.*/Mono.Cecil.dll; do
        if [ -f "$candidate" ]; then cecil=$candidate; break; fi
    done
fi
if ! command -v mono >/dev/null || ! command -v mcs >/dev/null || [ ! -f "$cecil" ]; then
    echo 'SKIP: Mono and Mono.Cecil 0.11 required'; exit 77
fi
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM
source="$repo/contrib/terraria-health/vanilla"
mcs -sdk:4 -target:library -out:"$scratch/G13TerrariaObserver.dll" "$source/Observer.cs"
mcs -sdk:4 -r:"$cecil" -out:"$scratch/Patcher.exe" "$source/Patcher.cs"
mcs -sdk:4 -out:"$scratch/Terraria.exe" "$repo/tools/terraria-fixture/VanillaFixture.cs"
before=$(sha256sum "$scratch/Terraria.exe" | cut -d ' ' -f 1)
mono "$scratch/Patcher.exe" --version
mono "$scratch/Patcher.exe" "$scratch/Terraria.exe" "$scratch/Terraria.G13.exe" "$scratch/G13TerrariaObserver.dll"
after=$(sha256sum "$scratch/Terraria.exe" | cut -d ' ' -f 1)
[ "$before" = "$after" ] || exit 1
if mono "$scratch/Patcher.exe" "$scratch/Terraria.exe" "$scratch/Terraria.exe" "$scratch/G13TerrariaObserver.dll"; then
    echo 'Patcher overwrote the original' >&2; exit 1
fi
G13MAP_HEALTH_FILE="$scratch/health" mono "$scratch/Terraria.G13.exe"
printf '%s\n' "$before" > "$scratch/G13Terraria.original.sha256"
cat > "$scratch/record.sh" <<'EOF'
#!/bin/sh
printf '%s\n' "$@"
EOF
chmod +x "$scratch/record.sh"
actual=$(sh "$source/launch.sh" "$scratch/record.sh" 'argument with spaces' "$scratch/Terraria.exe")
expected=$(printf '%s\n' 'argument with spaces' "$scratch/Terraria.G13.exe")
[ "$actual" = "$expected" ] || exit 1
printf '\n' >> "$scratch/Terraria.exe"
if sh "$source/launch.sh" "$scratch/record.sh" "$scratch/Terraria.exe"; then
    echo 'Launcher accepted a stale game copy' >&2; exit 1
fi
echo 'Vanilla Terraria patcher, CLR observer and launcher PASS'
