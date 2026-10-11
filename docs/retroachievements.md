# RetroAchievements on the LCD

The `retroarch` and `dolphin` readers show a game's achievement count, completion bar, current
hardcore/softcore mode and newly earned achievements this session. It uses
the emulator's local diagnostics and its existing login. No Web API key is needed.

## RetroArch

Create a profile and assign RetroArch's window class:

```sh
g13map profile create RA-RetroArch default
g13map profile health RA-RetroArch retroarch
g13map focus set retroarch RA-RetroArch
g13map focus on
```

For ordinary RetroArch launches, enable **Settings → Logging → Logging Verbosity**
and **Log to File**, then save the current configuration. **Timestamp Log Files**
is recommended when running several instances at once.
The equivalent `retroarch.cfg` entries are:

```ini
log_verbosity = "true"
log_to_file = "true"
log_to_file_timestamp = "true"
frontend_log_level = "1"
```

Quit all RetroArch instances before editing `retroarch.cfg` directly. A running
instance can save its older in-memory settings over file edits when it exits.
Alternatively, change the settings in RetroArch's menu and choose
**Configuration File → Save Current Configuration**.

Launch RetroArch normally and load a game. The reader finds the writable
`retroarch.log` or `retroarch__*.log` actually held open by the running native
emulator, including custom log directories. Timestamped logs give simultaneous
emulator instances separate files. An old log without a live writer cannot supply
stale progress. RetroArch's full diagnostic logs stay in its own configured log
directory; the reader does not copy them or change their contents.

Alternatively, launch through the capture wrapper without changing logging settings:

```sh
g13map ra launch
# Or pass the usual RetroArch arguments, including a core and content:
g13map ra launch -L /path/to/core.so /path/to/game
```

The launcher runs the installed `retroarch`, adds command-line logging overrides
and owns a private stdin/stdout pipe for game-specific memory readers. It retains
only recognized achievement lines under
`~/.local/state/g13map/retroarch/` (or `$XDG_STATE_HOME/g13map/retroarch`). It does
not copy login credentials or read the keyring. A temporary appended config enables
stdin commands and disables automatic config saving on exit for this launch,
keeping that pipe setting out of the normal config. Make permanent settings changes
in an unwrapped launch; manually saving a wrapped session can persist its temporary
settings. Existing appended configs are preserved. No RetroArch patch or plugin is
needed, and the launcher does not enable a network command server.
Keep the launcher running with the game. The reader checks the emulator PID and
process start time; an old log alone cannot keep a stale game on the LCD.

Unlocks show a trophy and title. Short titles hold for eight seconds. Long titles
wrap across two rows and scroll once within ten seconds, holding the beginning
and end for one second each. Further unlocks queue in arrival order. A notification's
timer begins when it is displayed. Each local trigger is eligible, including
intentional repeat triggers; there is no lifetime filter by achievement ID.

Progress updates on successful award confirmations. Repeated confirmations do not
inflate the count. Existing log records establish a starting baseline without
replaying old notifications. Switching games clears the previous game's queue.
`SESSION +N` counts confirmed achievements earned in the current game's session,
not their points. Returning to the profile rebuilds this count from the current
log without replaying notification cards.
The mode label follows RetroArch's active mode, including changes after loading.
Changing modes clears the previous mode's count until a new summary is available.
An unavailable mode is shown as `MODE ?`. The launcher never enables or disables
Hardcore; it leaves that choice and its enforcement to RetroArch.
The backlight keeps the profile's colour; completion is not treated as health.

The first version parses RetroArch's English summary log and the rcheevos game
and award diagnostics. Missing progress is displayed as `--/TOTAL`, never guessed.
Some modes omit the initial earned count; they receive no special handling.
Per-achievement points are omitted because the local award line lacks them.
Titles use the LCD's built-in uppercase font, substituting `?` for non-ASCII text.
Game-specific local displays are selected by RA game ID, with the general
achievement overview as the fallback.

[Battle Network 2 USA](battle-network-2.md) adds hidden Style vote, battle and
upgrade counters automatically through the shared `g13map ra launch`. The former
`g13map ra bn2 launch` command remains a compatible alias. The pipe is idle until
a supported game is detected, and its reader stops on unload or an unsupported
game. RA identity selects the adapter; a bounded signature check of directly
loaded ROM files also allows offline startup. Every memory sample still validates
the supported ROM layout and core memory mapping.

**Launch-time requirement:** a private pipe cannot be attached to an already
running, unwrapped RetroArch process. Such launches can still supply the general
achievement overview through native logs. A normal application-menu entry can
launch through the shared G13 wrapper transparently, using the same installed
RetroArch, cores, saves and login. Each additional game needs its own memory
recipe; the pipe alone does not provide universal game telemetry.

Input is bounded: 4 KiB per line, 256 characters per label, 64 KiB of log reads per
tick, and 10,000 queued titles. A full queue reports an error in the watcher journal.
Only one session owns the LCD. A live wrapper takes priority; otherwise the newest
native emulator with an open log supplies data. Native discovery runs every two
seconds; log reads and LCD updates continue at ten per second. Custom log filenames
from `--log-file` use the wrapper; native discovery recognizes RetroArch's standard
and timestamped filenames. To disable the display, run
`g13map profile health RA-RetroArch off`.

## Dolphin

Enable **View → Show Log Configuration**, select **Info** verbosity, enable
**Write to File**, and check **Achievements (RetroAchievements)**. Leave other
categories disabled to keep the log small. Quit Dolphin normally to save these
settings, then launch it normally and load a game. Its existing login is used.
Do not edit `Logger.ini` while an instance is running; it saves its in-memory
settings on exit. The corresponding saved values are:

```ini
[Logs]
RetroAchievements = True
[Options]
Verbosity = 4
WriteToFile = True
```

Create a separate profile and window rule:

```sh
g13map profile create RA-Dolphin default
g13map profile health RA-Dolphin dolphin
g13map focus set dolphin-emu RA-Dolphin
g13map focus on
```

The reader finds the running native `dolphin-emu` process's actual writable
`dolphin.log`, including custom user directories. It requires logging enabled
before launching and an initialization marker close to that process's start.
An old log without a live writer cannot supply progress. Dolphin appends to its
log; bootstrap reads at most the last 4 MiB, in 64 KiB batches, and waits until
that bootstrap finishes before showing progress. If the initialization marker
has fallen outside that window, quit Dolphin, archive its log and relaunch.
With multiple live Dolphin log writers, the reader waits rather than mixing
their records. One instance at a time is supported.

Dolphin's bundled achievement library reports active and total core achievements
per set. In ordinary sessions with Encore explicitly disabled and no unsupported
achievements, unlocked progress is total minus active, summed across reported sets.
If that inference is unavailable, earned progress stays `--`, with the total
shown when known. Mode changes invalidate the previous mode's count until a fresh
summary appears. Trophy cards, the FIFO queue, marquee holds, confirmation-based
progress updates and session reconstruction behave as described above. The
profile's backlight colour is retained. To disable it, run
`g13map profile health RA-Dolphin off`.

### F-Zero GX

The first game-specific display recognizes RA game **9699** and the USA disc
**GFZE01**, revision 0. It opens the running Dolphin instance's shared emulated
RAM read-only through `/proc/PID/fd`. Dolphin itself and the game are unchanged;
this needs neither an API key nor a launch wrapper. These are live RAM fields,
not a read of persistent SRAM or the memory card.

During a race, the LCD shows energy and speed in km/h. A circular sector fills
clockwise from twelve o'clock to the model's normal cruising speed. Above
102% it gains two CyberiaStyle-inspired upward chevrons only while the game's boost
timer is active, including dash plates. Downhill overspeed alone does not show
the arrow. On
results it shows the pilot name instead of the stale speed retained by the game.
Grand Prix context includes difficulty, numeric cup ID and race number; Time
Attack, Practice and Story have their own mode labels. A compact achievement bar,
count, percentage and `SESSION+N` stay visible. `HC` and `SC` mean Hardcore and
Softcore. Trophy notifications keep priority and return to the appropriate
display when their queue finishes. The race view abbreviates the session count
to `+N` to fit beside the gauge.

The game exposes a speed coefficient rather than a ready-made normal km/h limit.
The adapter estimates that limit from the current weight, acceleration, speed
coefficient and drag for every supported setup. A number such as `1093` in the
header gives the gauge's 100% reference in km/h. The model combines the propulsion
equilibrium with two fitted resistance parameters. Its coefficients remained
frozen through five independent machine tests, all within 1.5 km/h of observed
plateaus or independently extrapolated limits. Other machines and tuning values
still need validation.

`g13map ra fzero` inspects the telemetry, labels the limit as a model estimate and
shows any historical measured reference. `g13map ra fzero MEASURED_KMH` records
a measurement for comparison; it does not override the gauge. Those optional
records stay in `$G13MAP_CONFIG/fzero-speeds.conf` (normally
`~/.config/g13map/fzero-speeds.conf`) and bind machine ID plus the exact weight,
acceleration, speed-coefficient and drag values. The watcher calculates its limit
directly and needs no calibration file. Unsupported coefficients show dashes
with an empty gauge while keeping energy and km/h visible. No boosted peak or
arbitrary default is used as the normal cruising speed.

Other games, menus/replays, unavailable RAM and rejected samples use the general
achievement display automatically. Binding checks include the live process start,
shared-file identity, disc header/revision, dynamic pointer, real RAM bounds,
finite/ranged values and unchanged pointer/mode/flags across the sample. No previous
telemetry is held after a failed read. Emulators with a different shared-memory
layout may receive only the general overview.

Grand Prix racing and result flags, speed and pilot were checked against a live
USA session. The first racer block supplies actual speed, current/maximum energy
and boost state. Other race modes and damage/boost transitions still need physical
playtesting. Lap timers and live placing are omitted: a field named "Total Ranking"
did not match the player's displayed finish position. The adapter uses the same
RA totals as the general reader, including loaded subsets.

Telemetry field reference: [F-Zero GX RAM research](https://github.com/JoselleAstrid/fzerogx-docs/blob/master/addresses/racer_block_1.md).
