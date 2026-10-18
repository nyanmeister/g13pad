# ULTRAKILL health and combat statistics for the G13

A BepInEx plugin (the game is Unity, Mono, run through Proton on Linux) that writes
V1's state to the meter's feed file ten times a second: health with the hard-damage
cap, the style rank and its meter, the level timer, the dashes and the rail charge.
With `sprite v1` in the profile's own tuning lines the corner shows V1 instead of the
heart; every element appears because its word is in the line, nothing is switched.

The separate **Combat** display shows accuracy, average style points per minute,
and deaths, with health, style rank and level time beneath them. Both feeds are
written together; switching the profile back to Health retains the original view.

Accuracy counts fired attacks, once each. A pellet spread, piercing shot, ricochet
chain or explosion earns at most one hit for that attack when it damages an enemy.
Core-eject nukes credit both the core and detonating shot. Coin and magnet throws,
rocket-freeze toggles and ordinary fist/whiplash actions are neutral; hitting a coin
only earns credit when its chain damages an enemy. No shots or unavailable hooks
show `--%`. Counters restart with the level; checkpoint retries retain accuracy
and the death tally. STYLE/M uses the game's current level style points and timer,
including its checkpoint/score adjustments, rather than a rolling recent window.

## Install

OS packages include these files under `/usr/share/g13pad/ultrakill-health/`;
run that directory's `install.sh` if you do not have a source checkout.

With BepInEx 5 already installed, close the game and run:

```sh
./install.sh --game-dir "$HOME/.local/share/Steam/steamapps/common/ULTRAKILL" \
  --profile ultrakill
```

The script compiles against your installed assemblies with Mono `csc`, backs up
the previous DLL/config, installs the DLL and enables the stats feed. With no
`--game-dir`, it checks standard Steam paths under HOME. `--profile` is optional:
it selects Combat on an **existing** profile using a Combat-capable `g13map`.
Other options configure the BepInEx core, plugin/config destinations, the game-visible
health/stats feed paths, and the g13map executable (`install.sh --help`). Existing
health feed settings and unrelated configuration are preserved. No downloads,
package installation or root access. The script prints the backup location.
Backups live in `plugins.backups` beside the plugin directory: BepInEx recursively
loads DLLs inside `plugins`, even in hidden backup folders.

Select either view later:

```sh
g13map profile combat ultrakill on
g13map profile health ultrakill feed
```

`[Stats] Enabled` controls statistics collection. `[Stats] Path` overrides the
combat feed; empty writes `combat` beside the existing health feed, normally
`~/.local/state/g13map/combat` on the host. A Proton config path must be visible
inside Wine, for example `Z:\path\combat`. The watcher's default read paths are
the runtime combat file and the host state-directory combat file.

For a fresh BepInEx setup or manual installation:

1. BepInEx 5 (`BepInEx_win_x64_5.4.x.zip`, the Windows build, since the game runs
   under Proton) unpacked into the game folder, beside `ULTRAKILL.exe`.
2. In Steam, the game's launch options: `WINEDLLOVERRIDES="winhttp=n,b" %command%`, so
   Proton loads BepInEx's `winhttp.dll` proxy instead of its own.
3. `G13Health.dll` into `BepInEx/plugins/` (made there if missing). Build it with
   `build.sh GAME_DIR BEPINEX_CORE_DIR` and Mono's `csc`; it references the game's own
   `Managed/` assemblies, nothing from NuGet.
4. A profile in health mode `feed` with a window rule for `steam_app_1229490`, and in
   `~/.config/g13map/meter.d/PROFILE`:

   ```
   sprite v1
   heart 2 1
   ```

`BepInEx/LogOutput.log` says `[Info   :G13 Health] feeding Z:\home\... after
NewMovement.Update` once the plugin is up, then `component disabled` and `component
destroyed`: the game removes BepInEx's manager object soon after startup (found
2026-10-08: three launches with a plain `Update`, enabled and active, not one tick), which
is why the reading rides the player's own `Update` through a Harmony postfix and a timer
thread keeps the file alive; neither needs the component. The first launch with BepInEx also writes `BepInEx/config/BepInEx.cfg`.

Where the file goes: `[Feed] Path` in `BepInEx/config/g13pad.health.cfg` if set (a
Windows path; `Z:` is the host root under Proton), else from `HOME` (a native build),
else from Wine's `WINEHOMEDIR` (`\??\unix\home\NAME`). Proton passes no `HOME` into
the game (found 2026-10-08), so under Proton it is the config or `WINEHOMEDIR`; set the
config if the log says it found neither (it lists the environment it saw).

## What it reads

`NewMovement.Instance`: `hp` (0–200), `antiHp` (hard damage), `dead`, `boostCharge`
(0–300, the dashes); `StyleHUD.Instance`: the private `_rankIndex` and `currentMeter`
with `ranks[i].maxMeter` (reflection; names from the 2026-10 assembly); `StatsManager.
Instance.seconds` (the timer); `WeaponCharges.Instance.raicharge` (0–5). On the menu
there is no player: `wait`. The file is rewritten on change and once a second regardless
(`ttl 3`), and removed when the game quits.
