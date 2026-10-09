# Factorio native observer

`g13map-factorio` reads the native Linux client's memory through a read-only file
handle. It never attaches a debugger, stops the game, injects code, runs console
commands, changes a save, or installs a Factorio mod.

Supported executable: **Linux x86-64 experimental 2.1.21, build 87673**, ELF build ID
`60910b0b4f9cff6de7cf1a5a089334784f7945f8`. An unknown build leaves the game running
and reports that telemetry is unavailable. Offsets are not reused across updates.
Windows/Proton, ARM64, and headless servers are not supported by this reader.

Use Steam launch options:

```text
g13map-factorio %command% --nogamepad
```

Keep any other game arguments you already use. The wrapper must precede
`%command%`, so the actual game is its descendant; this gives the reader normal
user permission under Linux's restricted ptrace policy. There is no privileged
service and no change to that policy. The wrapper preserves argument boundaries
and returns the launched command's exit status even when telemetry is unavailable.

Create a profile in Health mode `feed` and a window rule for Factorio's window
class (inspect with `g13map focus windows`). The feed supplies:

- Character health/current effective maximum and total equipment shields/maximum.
- Total suit battery charge as `battery PERCENT`; absent without batteries.
- Current research as `research PERCENT technology PROTOTYPE_NAME`. Science,
  craft-item and craft-fluid research report progress; one-shot triggers report
  zero until completion. The name is shortened on the small LCD.
- `attack COUNT` for current `entity_under_attack` alerts across the local player's
  surfaces. The LCD shows ATTACK and fills the entire visible panel for 300 ms,
  immediately and then at most once every six seconds while alerts remain.
  Clearing alerts, changing their count, or switching window profiles cannot
  reset that cooldown.

Set `flash trace` in the profile's meter tuning file if personal low-health beats
should flash only the trace, leaving full-panel flashes distinctive for attacks.
The backlight still follows personal health. Research replaces the attack label
when alerts clear. Vitals remain available in remote view. Death reports zero;
menus/loading wait for a player. Paused games keep a heartbeat, and the feed
expires within three seconds after the wrapper ends.

The wrapper samples five times per second, follows its own process tree only
while finding the game, and walks the equipped items and alert records rather
than scanning the world or the heap. It writes on change and once a second to
`$XDG_STATE_HOME/g13map/health` (normally `~/.local/state/g13map/health`).
`G13MAP_HEALTH_FILE` selects a separate feed for testing.

For a permitted diagnostic read, `g13map-factorio --pid PID --once` prints one
feed line to stdout. It does not write a feed file or request elevated privileges.
An unrelated process may be denied by the kernel; ordinary play uses the wrapper.
`--version` and `--help` require no game, hardware, authentication, or display.

The private test scenario source is `tools/factorio/fixture/control.lua`. It is
never installed in Factorio's normal directories. It supplies known health,
shield, battery, research and attack values and exercises remote view and death.
Run it with a separate configuration, write-data directory, mod directory and X
display. The Steam build needs its app ID in the test working directory to avoid
redirecting a standalone launch back through Steam. Never run this fixture on a
normal save.
