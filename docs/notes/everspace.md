# EVERSPACE native observer and Ship display

**Verified against:** original EVERSPACE native Linux x86-64, Steam build 4246486,
game 1.3.5.36555, ELF build ID `a2470edeb6727aeaadf300bbbe68c515e18c008e`,
2026-10-18. Initial live hull/energy and no-shield state matched the HUD. Normal
Steam wrapper verified with zero effective/permitted capabilities and an updating
feed; automatic window/profile selection verified. The owner confirmed energy
drain and recovery readouts behave correctly in live play. Repair inventory was
read live; repair actions and shielded play still need live confirmation. Fixtures
cover their calculations. Newer native builds are
attempted with validated readings, never refused by build ID.

## Boundaries

`src/everspace_main.rs` launches the command Steam supplies, follows only its
descendants, and reads `/proc/PID/mem`. Keep the observer an ancestor of the game;
do not grant it privileges or relax ptrace policy. Telemetry failure leaves the
game running and preserves the launched command's exit status. `--pid` prints
diagnostics only. `--check-game` validates ELF/symbols, not member offsets.

`src/everspace/reader.rs` owns the native layout and bounded pointer/type/range
checks. `reader/stats.rs` derives repair capacity and conditional time estimates.
The watcher knows only the generic `ship 1` feed and its own `Display::Ship`;
game symbols and offsets never enter `meter` or `modes`. Ship and Combat share
`panel_feed::Live`, which polls at 100 ms and sends only changed pixels/colour.
The separate Ship painter uses the hull band directly, including steady red at
critical hull. It has no heartbeat, blink, death animation or wall-clock pulse.

## Native layout evidence

The tested executable includes full DWARF types. Derive offsets from those types
and accessors; the health, shield and energy components are not independent global
singletons. Resolve `GWorld`, `GUObjectArray` and the private native class globals
for ShipBase/Health/Shield/EnergyCore from ELF symbols on each new address space.
Stripped builds attempt the previous addresses. Bound section/string/symbol reads
and validate all resulting snapshots. Check memory permission before scanning the
symbol tables so a denied retry is cheap.

Follow `GWorld -> OwningGameInstance -> LocalPlayers[0] -> PlayerController.Pawn`.
Check the pawn's class ancestry against `AShipBase`, then each component's type.
Menus/null local pawn mean wait. Never mistake a menu actor for a damaged ship.

- Hull: current / maximum, with finite positive maximum.
- Shields: the component's weak device index **and serial** must match the current
  GUObjectArray entry. A missing/stale device means no shield. Current shield 0
  with a device is a depleted shield. Effective maximum is installed-device base
  capacity times `(1 + component bonus)` times its cached attribute multiplier.
  A dirty cached attribute is transiently unavailable, not a base-capacity guess.
- Energy: effective maximum subtracts allocated energy from the maximum attribute,
  matching `GetRatio(true)`. Check the cached attribute's dirty flag first.

An open memory file pins an address space. Probe its executable mapping even after
telemetry is disabled; reopen once it retires, including same-PID exec. Ten
consecutive invalid non-transient snapshots disable telemetry for that address
space. Validation is a best-effort check on newer builds, not proof of compatibility.

## Derived readouts

`FIX +N%` uses Health's nanobot pickup **class**, per-bot repair capacity, and
`GameInstance::GetResourceAmount`'s inventory-selection rules. Run resources are
the normal source; configured persistent resource classes outside hardcore use
CurrentPlayerData's resource array instead. Matching by display name or always
reading the run array gives plausible wrong inventories. Cap repair by missing
hull and floor to whole percentage points. Separately rounded current hull and
repair can sum to 99% when supplies actually suffice for full repair.

The shield hourglass counts **until recharge begins**, not until full:
`ceil(max(RemainingShutdownTime, RemainingRechargeDelay, 0))`. Both timers tick in
parallel. Summing double-counts; taking the smaller misses the extra delay after
depletion. At zero the label is `READY`, even while the bar is still refilling.

Energy uses `UWorld.TimeSeconds`, so pause does not change the estimate. Successive
energy changes estimate net consumption/recovery, smoothing while the sign stays
the same and resetting on sign changes or sampling discontinuities. A negative
estimate is seconds to empty. Positive is seconds to full; while regeneration is
delayed, include the remaining delay plus missing energy / cached output. Actual
observed recovery includes movement bonuses. Unlimited energy and a full core
suppress the estimate. These are conditional predictions, not guaranteed runtime.

The chosen layout keeps the energy percentage alongside `-Ns`/`+Ns`, with lowercase
seconds. Remove only the internal word gap when long numbers require it; timers
of at least 1000 seconds use rounded-up minutes (`m`). The inverse bolt flips
pixels against the pie, so it remains visible in both filled and empty sectors.

## Feed

```text
ship 1 hull 74 shield 38 energy 78 ttl 3 repair 12 shield_delay 8 energy_time -6
ship 1 hull 56 shield none energy 93 ttl 3 repair 43
```

Hull, shield, energy and positive finite ttl are required exactly once. Percentages
are whole numbers 0..999; shield and energy may be `none`. Optional repair is
0..100, shield_delay is 0..9999 seconds, and energy_time is -9999..9999 seconds.
Delay requires shields; energy_time requires energy. Unknown/duplicate keys and
oversize records are rejected. `wait ttl 3` clears stale values immediately.

Default paths: host state `g13map/ship` and the runtime ship feed. A configured
`G13MAP_SHIP_FILE` or `config_dir()/ship-feed` pointer is exclusive; never fall back
to an unrelated old default feed after a custom feed becomes unavailable. Bound
records at 512 bytes. The observer publishes atomically on change and once per
second, at 5 Hz sampling; expired/missing data yields dashes and resting colour.

## Installer and validation

### Analog input: verified local setup, 2026-10-18

The analog adapter was healthy, but EVERSPACE initially received no stick input.
A read-only physical trace measured both virtual axes through -32767..32767;
the failure was downstream. Steam loaded `controller_base/empty.vdf` for the game
and logged `xinput: false`. After enabling Steam Input and applying the existing
**G13 Analog** template to that controller, the owner confirmed game input works.
Steam recorded the device-specific selection, `xinput: true`, and reloaded the
same layout on game focus. The ordinary Gamepad template was viewed but not
independently verified in live play. No driver, service or calibration change
was needed. No input was injected and no device was grabbed.

For lateral strafe plus forward/reverse movement, use Gamepad **Strafe axis** X
and **Thrust axis** Y, with Y cleared from **Hover axis**. The settings can retain
both Y assignments, so check that separately after input recognition works.
The owner subsequently confirmed clearing that duplicate gives the intended
strafe/forward/reverse movement.

**Mouse crosshair recenters when the stick moves:** use the
[mixed mouse/analog setup](../../contrib/everspace-health/README.md#mouse-aiming-with-the-analog-stick).
The verified native-build workaround is the optional `mixed-input.sh` game patch,
with **Use touchpad to move crosshair OFF**. The owner tested movement, aiming,
menus, desktop pointer and return to flight. Preserve the working analog layout;
this is separate from Steam's earlier empty-layout failure.

Read-only tracing captured repeated gamepad-mode transitions during recentering.
DWARF/disassembly identify `FInputPreProcessor::EnableGamepadMode`; analog and
controller-key events call it. The patch makes that void function return before
changing mode or Slate hover state. It changes one byte on the known native build;
whole-file original/patched hashes guard apply/restore. This is **not** part of the
read-only observer or its installer and must never become an automatic game change.
The optional script stages replacements, verifies backups, rejects files in use,
and accepts custom game/backup paths. Unlike the read-only observer's optimistic
new-version reads, a code patch must reject uninspected builds.

Wrong paths worth retaining: Centered Crosshair and unused gamepad aiming-axis
unbindings did not solve this. Touchpad ON suppresses the mouse's mode switch-back
and initially appeared successful, but then broke the pointer in menus/other apps
while the game was visible. Touchpad OFF restored it. Native Display was already
Window; i3's fullscreen window state was not evidence of engine fullscreen.
The Linux Keyboard + Mouse UI has no UseSystemMousePosition option. Its real
save-data flag is bypassed by this controller's native never-hardware-mouse
default; no working INI override was established. Do not repeat Windows UI advice.

### Integration checks

Use [the recipient setup](../../contrib/everspace-health/README.md).
`contrib/everspace-health/check.sh GAME_DIR READER CLI` performs the actual installer
under a temporary HOME/runtime and complete private g13map paths. It checks default
Steam discovery, non-default paths with spaces, copying/preserving bindings,
unrelated window rules, backups/repeated installs, prerequisite rejection, and
executing the generated wrapper with literal shell metacharacters and child exit23.
No hardware daemon is needed. `--no-profile` and `--no-focus` keep installation
useful without requiring this machine's desktop arrangement.

The native fixture is laid out independently from DWARF offsets and hand-counted
resource values. It covers class inheritance, weak serials, dirty attributes,
nanobot inventories/hardcore, parallel cooldown timers, game-clock pause/rewind,
energy direction changes, fresh randomized ratios and moved/bounded ELF symbols.
Ship tests exercise bounded parsing, no-shield versus zero, inverse-bolt pixels,
custom-feed clearing and the actual private-FIFO worker holding steady critical
red across a full two-second blink cycle. The explicitly ignored frame exporter
`G13MAP_SHIP_DUMP=DIR cargo test --lib export_ship_frames -- --ignored` permits
visual inspection without writing the hardware feed. Full-suite ignored tests for
unrelated live emulator/audio sources are not evidence about EVERSPACE.
