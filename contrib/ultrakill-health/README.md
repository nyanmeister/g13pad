# ULTRAKILL feeder for the G13 health meter

A BepInEx plugin (the game is Unity, Mono, run through Proton on Linux) that writes
V1's state to the meter's feed file ten times a second: health with the hard-damage
cap, the style rank and its meter, the level timer, the dashes and the rail charge.
With `sprite v1` in the profile's own tuning lines the corner shows V1 instead of the
heart; every element appears because its word is in the line, nothing is switched.

## Install

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
