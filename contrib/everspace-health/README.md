# EVERSPACE: native ship display

Original EVERSPACE, native Linux x86-64. Verified against Steam build 4246486
(1.3.5.36555). Other native builds are attempted with validated readings. This
reader does not support EVERSPACE 2 or the Windows/Proton executable.

The observer reads its own game's memory without modifying game files, saves or
game state. It needs no mod, root access, debugger attach or ptrace-policy change.
The watcher shows hull, shields and energy with steady hull-band colour. It has
no heartbeat or critical-health blinking.

## Install

Build the observer and install a ship-capable g13map (0.3.7 or newer):

```sh
cargo build --release --no-default-features --bin g13map-everspace
contrib/everspace-health/install.sh
```

The OS package already includes the observer. With that installed, run
`/usr/share/g13pad/everspace-health/install.sh`; it finds the installed reader
without a source checkout. The optional `mixed-input.sh` is beside it.

The installer validates the native executable, installs `g13map-everspace` and
`g13map-everspace-steam` into `~/.local/bin`, and creates or updates profile
`everspace`. A new profile copies `default` when present; existing key bindings
are preserved. It selects Ship mode and adds a window rule for
`RSG-Linux-Shipping`. It does not restart services or edit Steam settings.

Set EVERSPACE's Steam launch options to the exact command the installer prints:

```text
/home/YOUR_USER/.local/bin/g13map-everspace-steam %command%
```

Retain other arguments after `%command%`. The wrapper must precede it: Linux lets
the observer read its own descendant game without granting broader access. The
game keeps running if telemetry fails; menus, stale feeds and unsupported layouts
show unavailable values.

Use `install.sh --help` for all options. For a non-default library/configuration:

```sh
contrib/everspace-health/install.sh \
  --game-dir '/games/Steam Library/steamapps/common/EVERSPACE' \
  --binary target/release/g13map-everspace \
  --bin-dir "$HOME/.local/bin" \
  --config-dir "$HOME/.config/g13map" \
  --profile everspace --base-profile gaming \
  --feed "$HOME/.local/state/g13map/everspace-ship"
```

`--no-focus` preserves window rules. `--no-profile` installs only the reader and
wrapper. `--g13map PROGRAM` selects the CLI, and `--window-class CLASS` selects a
different native window class. Existing affected files are backed up under
`$XDG_STATE_HOME/g13map/everspace-backups/` (default `~/.local/state`). Each backup
records the destination paths. Reinstallation preserves existing profile bindings.

To remove: clear the Steam launch option, turn Ship off on the chosen profile,
and remove its matching window rule with `g13map focus clear CLASS`. Remove the
two installed executables and `ship-feed` pointer if no other ship integration
uses them. Restore saved files from the timestamped backup to roll back an update.
The installer never changes the native executable or saves.

## Analog stick setup

The G13 profile's stick mode and Steam's layout are separate settings. For
continuous stick input, select analog mode in g13map, enable Steam Input for
EVERSPACE, and apply **G13 Analog** to the G13 controller. Open the layout browser
by clicking the current layout's name above **View Layout** / **Edit Layout**,
then choose **Templates**. See [Steam Input setup](../../docs/steam-input.md).

In EVERSPACE's Gamepad bindings, keep **Strafe axis** on left-stick X, assign
**Thrust axis** to left-stick Y, and clear left-stick Y from **Hover axis**. Use
separate keys for vertical hover. Avoid assigning the same axis to both hover
and thrust unless that combined movement is intentional.

### Mouse aiming with the analog stick

**Verified workaround for native Steam build 4246486 / game 1.3.5.36555:**
the optional [mixed-input.sh](mixed-input.sh) patch prevents automatic entry into
gamepad mode. The owner confirmed analog movement, off-center mouse aiming,
game menus and the desktop pointer all work, including returning to flight.
Keep **Use touchpad to move crosshair OFF**. The native input preprocessor
otherwise switches modes on controller events, which can reset mouse aim.

This deliberately changes the native game executable, separately from the
read-only telemetry installer. It also prevents automatic controller prompts
and gamepad menu mode. Quit EVERSPACE before applying or restoring:

```sh
contrib/everspace-health/mixed-input.sh --check
contrib/everspace-health/mixed-input.sh --apply
# Later, after quitting the game:
contrib/everspace-health/mixed-input.sh --restore
```

Use `--game-exe FILE` for another Steam library and `--backup FILE` to choose or
reuse an original backup. The default backup lives under
`$XDG_STATE_HOME/g13map/everspace-input-patch/` (normally `~/.local/state/`).
The script verifies exact original/patched hashes, retains the original, stages
and verifies replacements, and rejects an executable in use or an unknown build.
It needs standard coreutils, `flock` (util-linux) and `fuser` (psmisc), without
root. Steam verification or an update can remove the patch; check again before
reapplying. New versions require inspecting the game, not reusing an offset blindly.

**Failed alternatives:** Centered Crosshair off and clearing unused gamepad
yaw/pitch bindings did not fix recentering. **Use touchpad to move crosshair ON**
initially helped but subsequently hid/disabled the pointer in menus and other
applications while the game remained visible; turning it off restored the pointer.
The tested Linux UI has no “Use system mouse position” option. Preserve the
working G13 Analog layout; controller recognition is a separate issue.

## Readouts

- `FIX +12%`: whole percentage points of hull repairable now using carried
  nanobots, capped by missing hull. Rounding down avoids overstating supplies.
- Hourglass + `8s`: game seconds until shields can start recharging; depletion's
  longer shutdown counts too. `READY` means the wait ended, even while recharging.
  `NO SHIELD` means no shield device, distinct from a depleted shield at 0%.
- `78% -6s`: current energy and estimated time until empty at current net drain.
  `+6s` estimates recovery to full, including regeneration delay. Estimates use
  game time and remain conditional on current usage. Full or unknown recovery
  shows the percentage alone. Long values compact spacing; 1000 seconds and up
  use rounded-up minutes (`m`).

The feed is rewritten on change and at least once per second, with a three-second
expiry. A custom feed is recorded in the watcher's `ship-feed` configuration file.

## Checks and diagnostics

```sh
contrib/everspace-health/check.sh \
  "$HOME/.local/share/Steam/steamapps/common/EVERSPACE" \
  target/release/g13map-everspace target/release/g13map
g13map-everspace --check-game /path/to/RSG-Linux-Shipping
g13map-everspace --pid PID --once
```

`check.sh` confines all writes to a temporary HOME/configuration and tests default
Steam discovery, custom paths with spaces, binding/window-rule preservation,
repeat installation, backups, prerequisites, and the generated wrapper's argument
quoting and child exit status. It also executes a custom-feed wrapper. The ELF
check establishes executable/symbol compatibility, not live member validity.
`--pid` only prints diagnostics and requires permission already granted by Linux;
normal use is the parent launch wrapper above.

Development evidence and remaining live-test limits are in
[the integration note](../../docs/notes/everspace.md).
