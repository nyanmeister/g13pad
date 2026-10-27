#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Compiles the host-home resolution of both Terraria feeders (the lines between the
# host-home markers, taken from the real sources, not copied) with Mono's csc and runs
# hand-derived cases: HOME, WINE_HOST_HOME, the two WINEHOMEDIR forms, and the Windows
# profile that must never be taken for the host. No game, no display, writes private.
set -eu
if [ "${1:-}" = --version ]; then echo 'g13pad-terraria-home-check 0.1.0'; exit 0; fi
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
command -v csc > /dev/null || { echo 'Mono csc is required' >&2; exit 1; }
command -v mono > /dev/null || { echo 'mono is required' >&2; exit 1; }
scratch=$(mktemp -d /tmp/g13-terraria-home.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
for src in G13TerrariaHealth/HealthFeed.cs vanilla/Observer.cs; do
    name=$(basename -- "$src" .cs)
    # Observer keeps the method private; the harness class needs to reach it.
    body=$(sed -n '/host-home-begin/,/host-home-end/p' "$here/$src" | sed 's/private static string ResolveHome/internal static string ResolveHome/')
    test -n "$body"
    {
        printf 'using System;\nstatic class Feed {\n%s\n}\n' "$body"
        cat <<'EOF'
static class Check {
    static int failures;
    static void Set(string name, string value) { Environment.SetEnvironmentVariable(name, value); }
    static void Case(string home, string wineHost, string wineHomeDir, string want) {
        Set("HOME", home); Set("WINE_HOST_HOME", wineHost); Set("WINEHOMEDIR", wineHomeDir);
        string got = Feed.ResolveHome();
        if (got != want) { failures++; Console.WriteLine("FAIL HOME=" + home + " WINE_HOST_HOME=" + wineHost + " WINEHOMEDIR=" + wineHomeDir + " -> " + (got ?? "null") + ", expected " + (want ?? "null")); }
    }
    static int Main() {
        Case("/home/me", null, null, "/home/me");
        Case("/home/me", "/srv/other", @"\??\Z:\srv\other", "/home/me");      // HOME wins
        Case(null, "/srv/other", @"\??\Z:\srv\other", "/srv/other");           // then WINE_HOST_HOME
        Case(null, "relative", @"\??\Z:\srv\nt home", @"Z:\srv\nt home");     // a relative value is ignored
        Case(null, null, @"\??\unix\srv\unix home", @"Z:\srv\unix home");       // older Wine
        Case(null, null, @"\??\D:\srv\d home", @"D:\srv\d home");               // any drive letter
        Case(null, null, @"C:\users\steamuser", null);                          // a Windows profile is not the host
        Case(null, null, @"\??\Z:", null);                                      // too short to be a path
        Case("", "", "", null);
        Case(null, null, null, null);
        Console.WriteLine(failures == 0 ? "ok" : failures + " failures");
        return failures == 0 ? 0 : 1;
    }
}
EOF
    } > "$scratch/$name.cs"
    csc -nologo -out:"$scratch/$name.exe" "$scratch/$name.cs" > "$scratch/$name.build" 2>&1 || { cat "$scratch/$name.build"; exit 1; }
    printf '%s: ' "$src"
    mono "$scratch/$name.exe"
done
echo 'PASS: host-home resolution in both Terraria feeders'
