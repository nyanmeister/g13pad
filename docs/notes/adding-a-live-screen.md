# Adding a live screen (a new LCD mode / integration)

A live screen is a `Display` variant — `Health`, `Mpv`, `Pressure`, `Audio`, `Mpd`, `Agent` —
that paints the panel and the backlight while a profile is active, composed from the shared
kit. This is the recipe, worked from the agent screen (0.3.1), with the traps that only bit
on real hardware called out, because they will bite the next one too.

Written 2026-10-13..15 from the agent-screen build; dates in the text are the originals.

## The shape

- **A live screen is a `Display` variant, not a health reader.** Health mode is for a game's
  HP/shield. Anything else — a system readout, what a coding agent is doing — is its own
  `Display` variant composed from the kit (`meter/parts.rs` and `render.rs`: the glyph font
  `meter::text`, the `Draw` primitives, plus your own sprites). Do **not** bend it into the
  health meter's `Extra` bag; that is the trap in [integration-design](integration-design.md).
- **The best screen reveals what the monitor does not** — a peripheral-vision read (the
  colour, the beat, an alert you'd miss while looking elsewhere), not a re-print of what is
  already on screen. The agent screen leaves context% and model/effort off because the
  terminal's status line already shows them; it earns its place with the backlight alerts,
  the background-agent count and the is-it-alive pulse.

## The pieces (agent screen as the worked example)

1. **Model + feed**, if something outside feeds it: a module `src/<mode>/`. Define a one-line
   feed grammar (`parse`/`read`/`write`) the way the health feed does (see
   [feed-contract](feed-contract.md)); hand-derive the parse tests and the
   newest-valid-wins / `ttl` / `off` ordering. If several instances can run at once, key the
   feed per instance — the agent keys one file per terminal by its X11 window id
   (`$WINDOWID`), and `agent::select()` is focus-aware (the focused terminal's feed wins,
   with a sticky "latest" pointer that holds across a non-agent focus).
2. **Renderer** `src/<mode>/render.rs`: `frame(state, age, now) -> Bitmap` and
   `colour(state, age, now, rest) -> [u8; 3]`. `age` is time in the current phase (blink and
   breath timing); `now` is the wall clock (stopwatch, flash timestamps). Compose from the kit.
3. **Runtime** `src/<mode>/runtime.rs`: a frame thread like `meter::Live` — a
   `Shared { state, rest, resend, ... }`, a `run()` that draws a frame per `TICK`, sends the
   backlight (`daemon::send`, `rgb r g b`) only on change and the frame via
   `daemon::send_lcd`, and restores the resting colour on drop. `modes` owns the `Live` and
   feeds it; dropping it stops the frames.
4. **Wire `Display::<Variant>`**: `profile.rs` (the enum, `ALL` — bump its length,
   `label`/`about`/`line`/`parse`/`word`), `cli.rs` (`g13map profile <word> NAME on|off`),
   `modes.rs` (a `Panel` field, `show()` start/stop with the rest-colour handoff, a
   `feed_<mode>` poll beside `feed_meter`, and `refresh`'s `nothing_up`), `gui/screen.rs`
   (`group_of`). The compiler points you at every exhaustive `match` that needs the arm.
5. **A demo** (`g13map <mode> demo`, or a `demo` subcommand) that plays the states on the
   glass before any feeder exists — the thing to look at while tuning by eye.

The harness/game knowledge lives in the **feeder**, never the watcher: the agent's feeder is
`g13map agent feed <event>`, which turns a harness's hooks into feed lines and is silent /
always exits 0 so it cannot disturb the session. A second harness (Codex) writing the same
grammar drops in beside it.

## Traps that passed every unit test and only showed on the glass

- **Frame pacing — pace with `clock.frame_wait()`, never `clock.wait()`.** `wait()` is
  `min(sample_at, frame_at) - now`, and a frame-only thread never advances `sample_at`
  (only `sample_due()` does), so `wait()` returns **0**: the thread spins at full CPU and
  hammers the LCD turn-lock. That starves the main thread's profile-switch writes — the new
  display's first frame can't get the lock, so the **old frame sticks** ("stuck in <mode>
  mode" when you focus away) — and it wrecks blink timing. All 212 unit tests passed with
  this bug; a few seconds on the pad found it. (agent 0.3.1, 2026-10-15.) The meter paces
  the same way — copy it, don't improvise.
- **Backlight colours wash to white.** The G13's RGB backlight is a single additive LED, so
  any light in all three channels at once reads white-ish. Zero the channels a hue does not
  use — `[255, 200, 0]` yellow, not `[242, 208, 74]`. The pastel set looked right in the
  mockup PNGs (a lit pixel glowing on near-black) and washed out on the real lamp.
- **The version lives in two sources that must be bumped together.** `Cargo.toml`
  (`workspace.package.version`) drives the Rust binaries' `--version`; `CMakeLists.txt`
  `project(g13pad VERSION ...)` drives `PROJECT_VERSION` — the C/C++ binaries' `G13PAD_VERSION`
  **and** the `*-version` ctest checks. Bump only Cargo and `check()` fails eight tests with
  `Required regular expression not found. Regex=[<old>]`, while the C and Rust binaries
  disagree about their own version. (0.3.1, 2026-10-15.)

## Verify so the test can fail

- Break the code once and watch the matching test fail — the feed's off-wins-a-tie key, the
  selection's stickiness guard. A green test you never saw fail is not yet a test.
- Dump real renderer frames and look at them (the `mockup` skill tints a `P1` PBM to the
  backlight colour); the frame-content tests say nothing about how it reads on the panel.
- Run the actual `Live` thread on the private display or the pad. The pacing bug and the
  washed colours were both invisible to "212 passed" and obvious in seconds on hardware.
