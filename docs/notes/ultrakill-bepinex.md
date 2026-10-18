# Ultrakill bepinex

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** ULTRAKILL Steam build 22957324 under Proton with BepInEx 5,
2026-10-18. Health, revolver/coin accuracy, core-eject explosions and explosive
railcannon checked in live play; rocket/combined-nuke attribution has fixture
coverage only.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Unity games: BepInEx (ULTRAKILL)

`contrib/ultrakill-health` is the model: a BepInEx 5 plugin whose sampler rides
the player's `Update` ten times a second and reads the game's singletons.
What made it work:

- **Read the assembly, not memory.** `monodis Assembly-CSharp.dll` (Mono is packaged)
  lists every class and field; four of my six guessed names were wrong before I looked
  (`currentStyle` is `currentMeter`, the rank is a private `_rankIndex`, hard damage is
  `antiHp`, dashes are `boostCharge` 0–300). Private fields are one `GetField` with
  `BindingFlags.NonPublic` away; no Harmony patch is needed just to read.
- **Mono's `csc` builds the DLL** against the game's own `ULTRAKILL_Data/Managed/*.dll`
  and BepInEx's `core/` (`-nostdlib -noconfig`, reference `mscorlib`, `System`,
  `System.Core`, `netstandard`, `UnityEngine`, `UnityEngine.CoreModule`,
  `Assembly-CSharp`, `BepInEx`); no .NET SDK, no NuGet.
- **Proton:** the game is a Windows build, so BepInEx is the `win_x64` zip and Steam's
  launch option `WINEDLLOVERRIDES="winhttp=n,b" %command%` makes Wine load the doorstop
  proxy. The host's home is `Z:` plus the Unix path, but Proton passes no `HOME` into
  the game (the first launch said so); Wine's own `WINEHOMEDIR` (`\??\unix\home\NAME`)
  is there, and a `[Feed] Path` in the plugin's BepInEx config overrides either. When a
  path guess fails, log the environment's variable names: the next round is one launch.
- **The game destroys the plugin's component.** ULTRAKILL disables and destroys
  BepInEx's manager object soon after startup (the log says so once OnDisable and
  OnDestroy are logged); a plugin `Update` never runs, and it took three launches to see
  that the silence was the component, not the logger or the file. So: do the reading in
  a Harmony postfix on the player's own `Update` (`AccessTools.Method(typeof(NewMovement),
  "Update")`) and keep the file alive from a `System.Threading.Timer`; both outlive the
  component. Static state, a lock around the file. On an unknown game, log OnDisable and
  OnDestroy from the first build.
- **Menus have no player:** the postfix does not fire, so the timer writes `wait` once
  1.5 s pass without a frame. Write on change and once a second regardless, with
  `ttl 3`, so the meter ends with the game even if `OnApplicationQuit` never runs.
- A look per game is the profile's own tuning lines (`meter.d/PROFILE`: `sprite v1`,
  `heart 2 1`); the extra words draw their own elements, so no layout switch exists.

## Combat statistics (2026-10-18)

The selected C design is a separate `Display::Combat`, composed in `src/combat.rs`:
accuracy, average STYLE/M and deaths, with small HP, rank and level clock. It has
its own `combat 1` feed; the existing health feed and profile choice remain intact.
Game-specific attribution stays in the C# observer, not the watcher.

`Accuracy.cs` holds one ledger per level and one root per fired attack. Descendants
share a root; interacting attacks merge roots. A real enemy-health decrease
credits each root once. This covers pellets, split coins and core+rail nukes without
counting each victim/damage tick as another hit. Coin/magnet throws and utility
toggles are neutral. Level changes replace the ledger; late old projectiles keep
their old ledger. Checkpoint retries retain it. Zero shots/unavailable hooks show
unknown accuracy, rather than inventing 100%.

`CombatTracking.cs` uses Harmony scopes and weak attribution on spawned carriers.
Hook selection inspects calls once at startup: damage/spawn/cross-carrier callers
and **incoming transfer targets**, including callbacks that only schedule later work.
Dropping those apparently empty scheduling methods lost delayed coin attribution;
the real-Harmony fixture caught it. Unrelated per-frame methods stay unpatched.

Use `install.sh --help` for configurable game/core/plugin/config/feed paths and
optional existing-profile selection. `check.sh GAME_DIR [G13MAP]` tests actual
installation under a private HOME and custom destinations, configuration and
binding preservation, Windows path escapes, repeated installation, backups and
prerequisite failures. It also runs the ledger checks and real-Harmony fixture.
The fixture's Unity objects are substitutes; real game callbacks still need play
validation. Keep DLL backups **outside** BepInEx/plugins: the real loader scans
hidden subdirectories and otherwise discovers backup plugin versions.
