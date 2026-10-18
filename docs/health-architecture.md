# Health Mode developer map

**Telemetry sampling and LCD presentation have independent clocks.** The G13 can
display a new frame every **10 ms**, giving a **100 FPS ceiling**, as reported on
2026-10-10. The usual **100 ms telemetry poll** is a software sampling interval.
It does not limit event animations: an achievement or level-up event can start a
local sequence that renders intermediate frames without requesting new game data.
Actual throughput also depends on the driver, USB transfers and competing writers;
100 FPS is a ceiling, not a fresh hardware measurement from this refactor.

The custom driver's FIFO readiness wakes LCD processing independently of its USB
input read timeout. Keep those two paths separate. Skip missed presentation frames
after a slow write; never replay a backlog or multiply memory reads to animate.
`src/display_clock.rs` owns the sampling interval and minimum frame interval.

## Current source layout

```text
src/meter.rs                  stable Health Mode API and five reader identifiers
src/meter/
├── state.rs                  typed health, resource and extra-field values
├── feed.rs                   feed files, parsing, serialization and expiry
├── transport.rs              CS2 HTTP listener and Doom console-log follower
├── tuning.rs                 saved and live-reloaded meter preferences
├── parts.rs                  the kit: Heartbeat, Hold, Pie; parts no game knows about
├── render.rs                 glyphs, gauges, bars, the Meter composer and its per-game extras
├── runtime.rs                reader lifetime, output ownership and frame scheduling
├── demo.rs                   synthetic demonstration feed
├── source.rs                 Source-engine installation helpers
└── tests.rs                  existing behaviour and regression fixtures
src/fortress.rs               DF fortress overview presentation and DFHack installer
src/adventure.rs              DF local Adventure condition view
src/travel.rs                 DF travel needs and party view

src/ra.rs                     stable RA API, emulator source and shared limits
src/ra/
├── transport.rs              native sessions, diagnostic capture and bounded log tail
├── core_memory.rs            reusable bounded read-only private stdin/stdout protocol
├── achievements.rs           diagnostic parsing, totals, unlock queue and session gains
├── adapters.rs               static game registry, typed snapshots and epoch bindings
├── display.rs                trophy priority, overview and game-view selection
├── runtime.rs                sampling, session transitions and presentation clocks
├── bn2.rs                    BN2 layout validation, collection recipe and Style view
├── fzero.rs                  F-Zero layout validation, memory reader and race view
└── tests.rs                  existing RA behaviour fixtures

contrib/*-health/             external game collectors and their own install instructions
src/factorio_main.rs          standalone native Factorio memory observer
src/everspace_main.rs         standalone native EVERSPACE observer/launch wrapper
src/everspace/                validated native layout, ELF symbols and predictions
src/ship.rs                   generic ship feed and static hull/shield/energy page
src/panel_feed.rs             shared changed-frame worker for Ship and Combat
src/lcd.rs                    stored/imported animation playback and common bitmaps
src/daemon.rs                 serialized, atomic LCD/command writes
```

The existing `meter` and `ra` APIs and profile mode strings remain compatible.
Each module implements one responsibility; the facades re-export the existing API.
The shared feed remains the boundary for CS2, console logs and external collectors.
DF's three structured states use their dedicated renderers. RA retains its own
achievement model; it does not invent health or achievement state from game memory.

## The kit (`meter/parts.rs`)

What a screen shows is its own business; ideally what the game does *not* show (asked
2026-10-13: F-Zero's theoretical maximum, the share of it as a pie, the achievement
progress). What it shows it *with* is shared, and lives in `parts.rs`:

- `Heartbeat`: the scrolling ECG and its beat clock. `tick(pct, period)` scrolls and
  decides the pump; `trace(bm)` draws it; `just_beat(ticks)` and `swelling(ticks)` tell a
  panel when to flash and a sprite when to swell; `reset()` starts a fresh trace.
- `Hold`: the dark seconds after a drop to zero. `tick(zero, hold_ticks)`, `holding()`.
- `Pie`: a clockwise sector in a segmented ring, placed by a `Pie { cx, cy, disc, ring_in,
  ring_out, segments }`; `draw(bm, fill)`. F-Zero's speed dial is one.

`Meter` in `render.rs` owns one `Heartbeat` and one `Hold` and composes a health frame as
beat, sprite, rings, `extras_by_the_sprite`, readout, `extras_on_the_panel`, bars, flash.
The two `extras_*` hooks hold every per-game word (dashes, rank, mana, battery, research);
a new game adds to them, never to the composer. Pulling a part out of `Meter` is checked
by the frame exports: `G13MAP_METER_DUMP`, `G13MAP_FZERO_DUMP`, `G13MAP_RA_DUMP`,
`G13MAP_ADVENTURE_DUMP`, `G13MAP_TRAVEL_DUMP` and `G13MAP_MPD_DUMP` before and after must
be byte-identical (1016 files on 2026-10-13 when the heartbeat moved out).

## Composing a screen from the kit

For a new game, start from the feed line (docs/health-meter.md): most games need no
screen of their own, only their derived numbers in the line. Reach for a screen when the
game has something the monitor does not show (asked 2026-10-13: F-Zero's theoretical
maximum and the share of it, the achievement progress), and then:

1. Put the game's knowledge in its feeder or adapter, never in the watcher: the watcher
   receives numbers and words, and draws.
2. If the look is the health meter plus a few words, add the words to `Extra` and draw
   them in `Meter::extras_by_the_sprite` (beside the sprite, before the readout) or
   `extras_on_the_panel` (after it). Nothing else in `alive` changes.
3. If the look is its own screen, it is a `State` variant drawn from `Meter::frame_at`,
   composed from the kit: a `Heartbeat` when a pulse means something there (own one in
   the screen's state and call `tick`/`trace`), a `Hold` for a death, `Pie`, `gauge`,
   `bars`, `ring`, `text`, `readout`, `clock`, `gear`, the sprites. Colours come from
   `Tuning::rgb_at(pct)` and `alarm()`, never from constants of your own.
4. Count time in seconds from the tuning or in `TICKS_PER_100MS` units; a bare tick is
   20 ms. Animations keyed to `self.tick` must divide by it.
5. Before and after: freeze the frame exports, change, export again, `diff -rq`. A
   screen export of its own (`G13MAP_<NAME>_DUMP`, one saved frame per 100 ms) is the
   price of adding one, and what the next person's refactor will be checked against.

## Adding coverage

Use a `contrib` collector and the shared feed when a game already exposes telemetry
through a mod, plugin or observer. Reuse optional resource fields where their meaning
matches; introduce a typed state when the display describes a different domain.
Give feeds that can die an expiry, validate lengths/ranges, and preserve waiting,
death holds, tuning reload and profile handoff behaviour.

For an emulator game, add a descriptor to `ra/adapters.rs`, a typed snapshot/view,
and the adapter's bounded reader. The descriptor selects emulator source and RA
game ID; the adapter must independently establish supported region/revision and
layout. The private core-memory transport can be reused by another collection
recipe; it exposes bounded reads rather than arbitrary command strings. Register
that recipe on the descriptor and, if needed, a bounded direct-content recognizer
for offline startup. The shared launcher selects the recipe from emulator game
detection; it leaves unknown games idle and rejects collections spanning a switch.

**RetroArch memory integration requires launch-time pipe ownership.** Stock
RetroArch supplies the command protocol without a patch or plugin, but we cannot
attach that private stdin/stdout pipe to an already-running unwrapped process.
Each adapter also requires a core exposing its memory descriptors and a verified
game layout. A desktop entry may route normal launches through `g13map ra launch`.
Its temporary appended config enables stdin commands and disables automatic config
saving on exit; manual saves can persist temporary settings. Keep this caveat in
user guidance for every future RetroArch game adapter.

Snapshots bind to PID, process start, source, game ID and a monotonic runtime epoch.
The epoch changes when session/log identity or game/reset identity changes. A
same-ID reload advances a separate memory epoch without replaying trophy cards.
BN2 records the collection-start time; the registry requires a sample started
after its epoch boundary, so a recently published pre-reload sample is rejected.
Transport failures return
`Unavailable`; unknown games return `NoAdapter`; missing sessions return `NoSession`.
These are fallback outcomes, not permission to hold stale telemetry. Trophy cards
take priority, then a validated adapter view, then the ordinary RA overview.

## Presentation timing

The default telemetry clock remains 100 ms. BN2's independent capture recipe still
samples at up to five times a second. RA rendering caches only the most recent
validated sample between polling deadlines and checks its epoch before display.
Scrolling achievement cards request 10 ms presentation intervals; static views
request 100 ms. Future local event sequences can request the same 10 ms interval
without changing any collector's polling rate. Keep their duration based on elapsed
time, with bounded queues and explicit interruption/priority rules.

Health visuals draw 50 frames a second since 0.2.92 (`meter::TICK` is 20 ms; the glass
takes a frame every 10 ms, and 50 is the rate at which the trace moves exactly one
column per frame). The feed is still read ten times a second (`meter::POLL`): telemetry
and presentation are separate clocks. Every duration in the meter is seconds in the
tuning file turned into ticks at draw time; an animation that counts ticks counts them
in `TICKS_PER_100MS` units (5), so the gears still turn once in 4.8 s, the marquee still
pauses 1.2 s, the alarm flash still lasts a tenth of a second. The frame exports save
every fifth tick, so a contact sheet is still ten frames a second and the frozen sets
from before the change stay comparable.
Imported GIFs retain explicit delays down to 10 ms; shorter positive delays clamp
to 10 ms, while zero/unspecified GIF delays retain the 100 ms fallback. Stored
animation playback also clamps effective frame durations to at least 10 ms.

Stock RetroArch native logs still supply the general achievement overview. All
G13-wrapped launches now share the automatic memory transport; native unwrapped
launches do not acquire a pipe retroactively. No network command server is enabled.

## Regression checks after structural changes

Run the workspace suite and strict lint, including the lean feature build. Check
profile start/stop and same-reader handoff, feed expiry, listener lifetime, log
rotation, RA session reconstruction, trophy priority and adapter fallback. Timing
fixtures must establish that 100 animation frames can run with ten telemetry polls,
and that slow output skips missed frames. Hardware checks should cover both RA
adapters, CS2 and Doom, then representative shared-feed and DF states.
