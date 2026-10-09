# Dwarf Fortress fortress overview

Requires DFHack. The collector reads the loaded fortress; it changes no units,
items, jobs, weather, or temperatures. Tested with DF 53.16 and DFHack 53.16-r2.
It attempts to run on later versions. An incompatible API produces a waiting
feed and a DFHack error rather than invented healthy counts.

Install with the Dwarf Fortress directory, after installing DFHack:

```sh
g13map health dfhack "$HOME/.local/share/Steam/steamapps/common/Dwarf Fortress"
g13map profile health dwarf-fortress feed
```

Use your existing profile name in the second command and match its window rule
to Dwarf Fortress. Restart DFHack, or run `g13-lcd start` in its console.
The installer adds `hack/scripts/g13-lcd.lua` and the separate startup file
`dfhack-config/init/dfhack.g13-lcd.init`. It preserves existing startup files and
refuses to overwrite edited files or symlinks. Manual installation uses those
same paths, with `g13-lcd start` as the startup file's command.

## Reading the LCD

The 160×43 display keeps a fixed overview and a scrolling detail line:

```text
[dwarf] 116  RAIN      8°C
CARE 3 (1 INF)
LIMBS 2 (W1 H2)
CLOTHES 11 (3 RAGS)
--------------------------
TON ... - INFECTION
```

- **Dwarf icon and population**: living fortress citizens and residents on the map, including insane
  citizens. Uses DFHack citizenship/residency, including modded races.
- **CARE**: citizens with a healthcare request, wound infection, or an active
  syndrome marked sick. Counted once per citizen, however many issues they have.
- **INF**: citizens with positive infection level or an infected wound, included in CARE.
- **LIMBS**: unique people with impaired standing or grasping limbs. **W / H**
  give the walking/hand breakdown; someone with both counts once in the total.
  Impairment means fewer functioning standing/grasping limbs than the creature's
  maximum. Includes partial impairment, not only complete inability to walk.
  An impaired squad member gets priority in the detail line.
- **CLOTHES**: citizens wearing at least one unarmoured clothing item with wear ≥1.
  **RAGS**: wear ≥2 (tattered), a subset of CLOTHES. Wear ≥3 also counts. Babies are
  excluded from clothing counts; multiple bad items still count as one wearer.
- Weather and Celsius temperature come from a small grid of revealed outdoor
  tiles. Temperature is the median; the coldest sample determines the freezing
  warning. Readings are current observations, not a forecast. They cannot
  guarantee that a particular pool or river will freeze. No revealed samples
  yields unknown; disabled simulation yields OFF/unknown.

The detail line cycles between the highest-priority health issue, hospital
shortages, active strange moods, and freezing warnings. Messages that fit remain
for eight seconds. Scrolling messages pause for one
second at the beginning, scroll their full text, and pause for one second at
the end before the next phase, including when a lone message repeats.
Up to three hospitals and three moods appear, ordered by urgency. Existing
infection or care requests do not permanently hide the other categories.

**Hospital shortages** use each active hospital's own stock counters and
configured targets: soap, thread, cloth, splints, crutches, plaster, and buckets.
`EMPTY` means none stocked; `LOW` means below target. Splints, crutches, and
buckets show piece counts such as `3 OF 5`; thread, cloth, soap, and plaster use
the game's internal quantity units, so the LCD shows EMPTY/LOW instead of
misleading item counts. The hospital name follows the shortages. Relevant
healthcare request counts are fortress-wide, not patient assignments to that
specific hospital, and do not prove why a treatment job is blocked. An empty
supply needed for current requests is shown even if its configured target is zero.
No active hospital produces a separate warning. Stocked supplies do not establish
staffing, water access, or treatment readiness.

**Strange moods** show the required workshop before it is claimed, then exact
materials and quantities still to be collected. Silk, plant fiber, yarn,
specific metals, bones, and shells retain their distinctions. This follows the
job requirements used by DFHack's `showmood`; it does not assume that an item
in Stocks is reachable or available. Once collection is complete, the detail
reads `MAKING ARTIFACT`. Mood status is read-only; no mood is triggered or altered.

The normal backlight follows the in-game calendar season:

| Season | R | G | B |
| --- | --- | --- | --- |
| Spring | 255 | 0 | 120 |
| Summer | 33 | 234 | 0 |
| Fall | 249 | 28 | 0 |
| Winter | 0 | 188 | 163 |

Persistent health issues do not hide the season colour. The configured alarm
colour briefly overrides it during an urgent-event LCD flash. Newly observed
urgent cases or a transition into freezing trigger one 300 ms full-LCD flash,
with at least six seconds between flashes. Existing problems at load are a
baseline; persistent problems do not repeatedly flash. Seasonal changes do not
trigger an alert. Older feeds without a season retain their urgency colours.

## Lifecycle and diagnostics

Collection runs every two seconds; the sampled environment refreshes every
five seconds. A frame timer rewrites the feed every second even while paused
and on the title screen. Save changes reset cached coordinates and event
baselines. Menus and non-fortress modes publish `wait ttl 6`. A crash or exit
expires the feed within six seconds.

The path is `$G13MAP_HEALTH_FILE` when set, otherwise
`$XDG_STATE_HOME/g13map/health`, defaulting to `~/.local/state/g13map/health`.
The home path works through Steam's container. Writes are atomic. The watcher
accepts bounded `fort 2` records independently of HP rendering. Version 2 adds
the unique impaired-person count and a detail phase counter; version 1 remains
readable with W/H displayed without an invented total. Upgrade the watcher before
starting the new collector.
New collectors also include a validated calendar season; older version 2 records
without this field remain readable.

DFHack commands: `g13-lcd status`, `once` (one protocol line), `inspect`
(JSON with affected citizens, hospital shortages, and mood requirements), `start`,
`stop`, and `--version`. Start is idempotent; stop cancels the timer and only clears
its own current feed.

To remove, run `g13-lcd stop` in the running game first, then:

```sh
g13map health dfhack "/path/to/Dwarf Fortress" remove
```

Removal preserves edited files. For a different release, back up the old
managed files and remove them before installing the new copies.
