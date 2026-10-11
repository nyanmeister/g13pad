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
├── render.rs                 gauges, heartbeat, colours and legacy visual timing
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
src/lcd.rs                    stored/imported animation playback and common bitmaps
src/daemon.rs                 serialized, atomic LCD/command writes
```

The existing `meter` and `ra` APIs and profile mode strings remain compatible.
Each module implements one responsibility; the facades re-export the existing API.
The shared feed remains the boundary for CS2, console logs and external collectors.
DF's three structured states use their dedicated renderers. RA retains its own
achievement model; it does not invent health or achievement state from game memory.

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

Legacy health visuals keep their existing 100 ms logical step: beat speed, swell,
death holds and vehicle animation must not accelerate when faster event rendering
is added. Their logical step and a future event renderer's frame rate are distinct.
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
