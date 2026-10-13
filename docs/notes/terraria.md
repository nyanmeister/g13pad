# Terraria

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** tModLoader 2026.08.3.0 / Terraria 1.4.4.9 native, vanilla Terraria 1.4.5.8 under Proton, 2026-10-09.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Terraria: distinguish the two runtimes

`contrib/terraria-health` contains a client-only tModLoader mod (.NET 8) and a
separate vanilla Windows CLR observer (.NET Framework 4). Native tModLoader may
use `dotnet` as its window class; Proton uses the Steam application class. The
editor owns the LCD while open, so close it before judging the live meter.

Read player fields after the game's update, including effective health/mana
maxima, and publish from a separate timer so actual paused updates keep a live
heartbeat. The vanilla patcher preserves the original executable and branch
targets at all normal returns, widening short branches. Cecil needs both embedded
game assemblies and Wine Mono's XNA/FNA metadata to rewrite the assembly. Test
with `tools/check-terraria-vanilla.sh`; never install the gameplay fixture mods
into ordinary saves. A stale executable copy must be rejected after game updates.
Pass the host feed path explicitly through `G13MAP_HEALTH_FILE`: .NET Framework's
environment can hide HOME/WINEHOMEDIR even when the host process has them. Windows
GUI console handles can discard errors; log asynchronously beside the copy.
