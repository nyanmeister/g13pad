# Zero-K: off-screen alerts on the glass

Built 2026-11-04 (Claude, Fable 5.1) from the design picked 2026-10-27 (compass, option B
of `G13 Review/mockup-20261027`). First RTS integration.

## What it is

`Display::Alert` (`src/alert.rs`): a compass dial on the left, a thick arrow to the
last thing hit or lost out of view, its name at the big size, `HIT SW` / `LOST NE`, how
long ago, and severity bars at the right edge. The backlight flashes red (severity 4–5),
orange (2–3) or yellow (1) for three seconds after the event, then the profile's colour
comes back while the arrow stays until the feed's `ttl` runs out. `QUIET` with a bare dial
is a live feeder with nothing to say; `NO FEED` is no feeder. It rides on
`panel_feed::Live` like the combat and ship screens (100 ms poll, changed pixels only).

## Feed

One line, `alert BEARING LABEL [kind hit|lost] [sev 1-5] [at EPOCH_MS] [ttl S]`, or
`quiet [ttl S]`, or `off`, in `$XDG_RUNTIME_DIR/g13map-g13-0.alert` or
`~/.local/state/g13map/alert`. Bearing is degrees clockwise from the map's north. `at`
carries the event time so keepalive rewrites do not reset "ago". `g13map alert feed LINE`
writes one; `g13map alert demo` plays four states over twenty seconds.

## Zero-K side (`tools/zero-k/`)

- `g13pad_alerts.lua`: a LuaUI widget. `UnitDamaged` of our team off screen
  (`Spring.IsUnitInView`) becomes `hit`; `UnitDestroyed` of anything costing 120+ metal
  becomes `lost`. Severity from `metalCost` (120/300/800 steps), the commander
  (`customParams.commtype`) is 5 and labelled `COM`; a loss adds one. Bearing is from
  the ground point under the camera's line of sight (`GetCameraPosition` +
  `GetCameraDirection` projected to y=0), so the arrow means "from where you are looking".
  Cheap units being plinked (severity 1) do not alert; a unit repeats after one second;
  a higher alert holds the glass for three seconds against lower ones. Keepalive `quiet`
  every two seconds, `off` on shutdown or going spectator.
- LuaUI's `io.open` only reaches Spring's write directory, which is the Zero-K install
  folder (proven 2026-10-27 with `/keysave`). So the widget writes
  `<Zero-K>/g13pad_alerts.txt` and `g13map alert relay FILE` copies each change into the
  feed, validating it through the same parser first. The relay is a user service,
  `g13map-zerok-relay.service`, installed by `tools/zero-k/install.sh` (which also copies
  the widget into `<Zero-K>/LuaUI/Widgets/`); `--uninstall` removes all three.
- The profile: `g13map profile alert zero-k on`; the focus rule `spring → zero-k` puts it
  on the game's window.

## Verified

- Hand-derived parse tests, the sector table at its boundaries (337 is NW, 338 is N),
  the arrow tip pixel for a SW bearing (mutant: flipping the arrow's x failed the test),
  colours flash then rest, frames dumped through the renderer and looked at
  (`G13MAP_ALERT_DUMP=DIR cargo test --lib alert::dump -- --ignored`).
- Relay end to end: a line written to `<Zero-K>/g13pad_alerts.txt` appeared in the runtime
  feed within 600 ms. Demo played on the pad under the zero-k profile.
- Not yet verified: the widget inside a real game (loading as a local widget, the
  `os.time` and camera calls, the volume of alerts in play). First game decides that.

## Later

- Option C (minimap with the camera rectangle) as a second mode for advanced play; A
  (word stack) for several alerts at once.
- The widget could use `Spring.GetUnitDefID` groups for better short names, and skip
  units already under attack for ten seconds so a base under siege reads as one alert.
