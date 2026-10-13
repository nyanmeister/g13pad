# Factorio

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Factorio 2.1.21 native Linux x86-64, build 60910b0b…, 2026-10-09.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Factorio: observe a verified native executable

`src/factorio_main.rs` is a read-only external Rust observer and launch wrapper,
not a Factorio mod. Linux ships function symbols and line DWARF, but not the game
class layouts: gdb's initial indexing is costly and does not recover those types.
Derive fields from named accessors and resolve relocated globals and class markers
from ELF symbols. The build ID is informational: try new native builds with the
last member layout, validating pointers, object types, bounded collections and
finite numbers. Keep HP bounds generous for mods and permit overheal/overcharge.
Ten consecutive bad snapshots disable telemetry for that address space; menu/loading waits
reset that counter. A mod-sync restart can exec the same binary with the same PID:
an open /proc/PID/mem handle still refers to the old address space and returns EOF.
Probe the executable mapping even after disabling telemetry and reopen only when
that handle retires; do not confuse stale memory with incompatible game data.
Verify same-PID exec, changed IDs, moved symbols and junk data in a private
scenario. Read `/proc/PID/mem` only;
never loosen ptrace policy or install a privileged observer. The production
wrapper is the game's ancestor, including Steam's runtime wrappers.

The equipment vector contains every equipped item. Classify shield/battery
objects before reading their fields; an empty armor inventory has no grid. Health
is a ratio multiplied by the effective maximum (quality and force/character
bonuses). Use the local player's force, and retain their character controller
while remote view is active. Research can be science or a trigger; craft-item and
craft-fluid triggers have separate counters. Experimental 2.1 makes
`current_research` read-only: the fixture queues it with `add_research`.

CharacterController::getVehicle reads character+0x550. Vehicle health is a ratio
at +0x80, with prototype+0x48/max+0x630 and quality+0x91. Resolve and validate
Car/train/spider class markers before reading, and verify getting out, passenger,
quality and remote transitions in the private vehicle fixture. Vehicle HP becomes
primary; suit shield/battery stay personal, and the small pilot HP remains visible.
Do not apply the character's force/individual HP bonuses to a vehicle maximum.
Vehicle rendering suppresses ECG and death holds from a destroyed vehicle.
The default vehicle icon is a chain of three counter-rotating gears; damage cracks
follow the upper edge of the named orange band, with 50% as the fallback. Keep
cracks on the rotating bodies and clear them when health recovers above the band.
Research labels are bounded to 256 ASCII characters and marquee inside a clipped
window with fixed progress; keep byte/glyph bounds and long-uptime animation tests.

The attack signal is `entity_under_attack`, not every alert category. Its renderer
owns a monotonic six-second cooldown so clearing/reappearing alerts cannot retrigger
it early. Check actual text glyphs too: the old small font contained only style
and Terraria letters, turning BAT/ATTACK/research names into dashes. An exported
frame caught that despite passing compilation and unrelated meter tests.
