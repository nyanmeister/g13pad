# g13pad: how to work here

The per-area runbooks moved to `docs/notes/` on 2026-10-13; this file keeps the nudges.
Read the note for an area before touching it:

| Area | Note |
|---|---|
| The feed line, where a feeder may write, choosing the profile | [feed-contract](docs/notes/feed-contract.md) |
| Games: DRG/UE4SS, CS2, Doom engines, GoldSource, Source, Terraria, ULTRAKILL, Factorio, Dwarf Fortress, Noita | [deep-rock-galactic-ue4ss](docs/notes/deep-rock-galactic-ue4ss.md), [counter-strike-2](docs/notes/counter-strike-2.md), [doom-engines](docs/notes/doom-engines.md), [goldsource](docs/notes/goldsource.md), [terraria](docs/notes/terraria.md), [ultrakill-bepinex](docs/notes/ultrakill-bepinex.md), [factorio](docs/notes/factorio.md), [dwarf-fortress-dfhack](docs/notes/dwarf-fortress-dfhack.md), [noita](docs/notes/noita.md) |
| RetroArch and Dolphin readers | [retroachievements-readers](docs/notes/retroachievements-readers.md) |
| System tools: audio, microphone, MPD, pressure | [system-tools-and-audio](docs/notes/system-tools-and-audio.md), [mpd-music-companion](docs/notes/mpd-music-companion.md) |
| Controller bindings | [controller-bindings](docs/notes/controller-bindings.md) |
| Demos, telemetry and display clocks | [telemetry-timing-and-demos](docs/notes/telemetry-timing-and-demos.md) |
| OBS overlay and the driver's state files | [obs-overlay](docs/notes/obs-overlay.md) |
| A program on a key (run bindings) | [run-bindings](docs/notes/run-bindings.md) |
| A new live screen (LCD mode / integration) | [adding-a-live-screen](docs/notes/adding-a-live-screen.md) |
| Iterating without the package cycle | [iterating-without-packaging](docs/notes/iterating-without-packaging.md) |

## The shape

- **The watcher knows a feed grammar, not games.** A game's knowledge belongs in its
  feeder (`contrib/`, or its own binary like `g13map-factorio`). If a change adds a game's
  name to `src/meter` or `src/ra`, stop: that is the open design question in
  [integration-design](docs/notes/integration-design.md) (the 2026-10-12 review's five
  findings are one question, and the answer is not another field in `Extra`).
- **Integrations are read-only, so they attempt newer game versions** (asked 2026-10-13).
  "Verified against" is a line at the top of each game note, never a gate in code. A
  reader that a newer version defeats shows an honest empty display and the user turns
  the LCD to another mode. Factorio is the model: a `BUILD_ID` mismatch is logged and the
  last layout is tried under validation. The F-Zero reader once refused any disc revision
  but the one it was written against; it no longer does.
- **A screen is composed from the kit** (`meter/parts.rs`, `render.rs`; the steps are in
  docs/health-architecture.md). Per-game words go in `Meter`'s two `extras_*` hooks. A
  bare tick is 20 ms since 0.2.92: count animations in `TICKS_PER_100MS` units, and
  check a change against the frozen frame exports, sampled every fifth tick.

## Before believing a check

- Every real bug so far sat at a boundary no unit test reached: a container's private
  `/run/user`, an engine's string table, a tar under a `.pk3` name, a timestamped log
  name, sway's `app_id`, a frame thread paced by `clock.wait()` instead of `frame_wait()`
  (it spun, hammered the LCD lock and stuck the display — 212 tests green, found in seconds
  on the pad; see [adding-a-live-screen](docs/notes/adding-a-live-screen.md)). Reach for the
  empirical check at the boundary first: a private display, a grep of the live log, a frame
  dump looked at, the `Live` thread actually run.
- Report passed *and* ignored. The ignored tests are the boundary ones (live RAM, the
  Dolphin log, the audio server, the mutation fuzzers); run the one whose boundary the
  change touches and say so.
- `g13map edit` execs the sibling `g13map-editor`: build both bins and check `--version`
  or the mtime. After an install, check the service's `ExecStart` is `/usr/bin/g13map`;
  a `dev-watch.sh` drop-in once pinned it to a stale `/tmp` binary for days.
- Read the assembly or header dump, never guess a member name: four of six guessed
  ULTRAKILL names were wrong. For DF, inspect the live fields in a copied fortress.
- A changed mod loads on the next game launch (UE4SS, BepInEx); a silent plugin is more
  often a destroyed component than a broken logger. Log OnDisable/OnDestroy from the
  first build.

## Never on the live machine

- An isolated editor or watcher needs the whole `test_support::Sandbox` set:
  `G13MAP_CONFIG`, `G13MAP_PIPE`, `G13MAP_OUT_PIPE` (with a reader draining it),
  `XDG_RUNTIME_DIR`, `G13MAP_UNIT=0`, `G13MAP_ANALOG=1`, `G13MAP_ANALOG_MAP`.
  `XDG_CONFIG_HOME` alone pushed a picture to the owner's pad for a minute (2026-10-12).
- Never mute the owner's microphone, touch the live MPD queue or ratings, reroute audio,
  or enumerate the share to test a control. Private servers, null sinks, fake FIFOs.
- Headless compositors and OBS write into the live session's environment or config
  unless `XDG_*` and `dbus-run-session` are set up first; check the process's environ.
- Xvfb :99 is the project's private display. `pgrep -a -x Xvfb` before starting one;
  kill by PID. A `pgrep -f` on your own pattern matches your own shell.
- Steam's DFHack launcher runs `pidof dwarfort`: a private copied DF blocks the owner's
  launch. Stop it and verify the exit; SIGTERM was ignored once.
- A `systemctl restart lightdm` or session switch on the VM: say it before the command.

## Build, test, package

- Package under a path without spaces (`/tmp/g13-package-VERSION`): `LD_PRELOAD` splits.
- `lint && build` under `set -e` leaves the old binary in place; check the version after.
- Tests that mutate the process environment go through `test_support::Sandbox`.
- The CS2 listener's loopback bind can fail in a sandbox with `Operation not permitted`;
  rerun with localhost access rather than changing the listener.
- Every binary we ship answers `--version` without hardware or a GUI.
- The version lives in TWO sources that must move together: `Cargo.toml`
  (`workspace.package.version`, the Rust binaries) and `CMakeLists.txt` `project(... VERSION)`
  (`PROJECT_VERSION`: the C/C++ binaries' `G13PAD_VERSION` and the `*-version` ctest checks).
  Bump only Cargo and `check()` fails with `Regex=[<old>]`; the C and Rust binaries then
  disagree about their own version. (0.3.1, 2026-10-15.)
- The G13 takes a frame every 10 ms; a 100 ms poll does not limit animation. Collectors,
  presentation clocks and USB timeouts stay independent; skip missed frames.
- A `cmd &` load generator in a test command outlives the call. Kill its PIDs in the same
  command (`trap 'kill $pids' EXIT`) and `pgrep` for leftovers after a contention test:
  nine `taskset -c 0 sh -c 'while :; do :; done'` shells from the 2026-10-13 overlay timing
  run spun on core 0 for seven and a half hours.
