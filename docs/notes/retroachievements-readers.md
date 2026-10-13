# Retroachievements readers

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** RetroArch 1.22.2 / mGBA with Battle Network 2 No MP Hard Mode v1.1, 2026-10-10; Dolphin with F-Zero GX GFZE01 revision 0, 2026-10-09.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## RetroAchievements: general reader and game adapters

RetroArch game-specific memory integration needs private stdin/stdout pipes owned
at launch by `g13map ra launch`; they cannot be attached to an already-running
unwrapped emulator. Stock RetroArch's existing command support needs no patch or
plugin. The shared launcher detects supported games, selects registered collection
recipes and invalidates in-flight samples on switches/reloads. Each adapter still
needs its own verified layout and a core exposing memory descriptors. Preserve
this caveat in future game documentation. Temporary stdin/config-save overrides
apply only to the wrapped session; manual config saves can persist them. No network
command server is enabled by the wrapper.

Keep trophy cards ahead of game-specific telemetry, and retain the general RA
overview when an adapter rejects data. Dolphin's log reports active/total counts
per achievement set; sum distinct sets, and keep unknown earned counts unknown
for unsupported or Encore sessions. Never infer an unlock from a changed total.

F-Zero GX USA uses the shared emulated RAM backing file read-only, with process
start/FD identity, the disc id (any revision is attempted since 2026-10-13), dynamic pointers and retail RAM bounds.
The first racer block supplies speed/energy and the boost timer; global HUD speed
is stale after results. The normal-speed coefficient is not km/h, dynamic base
speed is not a ceiling, and the maximum-seen field includes boosts. All supported
setups use the frozen empirical normal-speed model, with its numeric reference
on the LCD and model provenance in the diagnostic CLI. Historical measurements
never override the gauge. Guard its coefficient domain; unsupported values keep
an empty gauge marked with dashes. Preserve timestamped predictions and compare
with independent plateau/curve references, labeling the distinction. The eight-setup
fit passed five held-out machine tests; arbitrary modified physics remains unverified.
Bind measurements to weight, acceleration, speed coefficient and drag as well as
machine ID. An expired boost
can leave speed well above normal; the requested arrow requires both an active
boost timer and speed above 102%. Include that case in live checks.
