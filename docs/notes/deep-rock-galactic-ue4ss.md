# Deep rock galactic ue4ss

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Deep Rock Galactic with UE4SS, 2026-10-07 (game version not recorded);
reads confirmed again on Steam build 26107973 (2026-10-22) with UE4SS 3.0.1 under GE-Proton
11-11 on 2026-10-26 (health 110, armor 25 in UE4SS.log), the write path fixed the same day
and confirmed live at the next launch (23:58: `loaded; feed Z:\home\...\health (feed.txt)`,
then `110.0/110.0 shield 25.0/25.0 ttl 15` on the host within a minute).
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Reading health out of an Unreal game (UE4SS)

`contrib/drg-health` is the model: a UE4SS Lua mod polling the local pawn five times a
second. What made it work:

- **Find the member names in a header dump**, not by guessing. For Deep Rock Galactic
  the community publishes `DRG-Modding/Header-Dumps` on GitHub (`Current/FSD.hpp`, with
  `FSD_enums.hpp` for the enums). The health component there has `GetHealth()`,
  `MaxHealth`, `GetArmor()`, `GetMaxArmor()`, `IsDead()`, and the shield is "armor".
  Other games have similar dumps or a UE4SS object dump can make one.
- **Track the pawn with construction events** (`NotifyOnNewObject`) and check
  `IsLocallyControlled()`; never scan all objects on a timer, loading screens churn
  objects and the poll will stutter the game. Everything that touches a UObject runs
  inside `ExecuteInGameThread`, under `pcall`.
- **The 100% point is not the first maximum you see.** On DRG's Space Rig the pawn
  reports its maximum without perks; the mission dwarf reports it with them. "The
  lowest maximum seen" therefore took the rig's and read a perked dwarf as 114%. The
  rule that holds: the game's current maximum with temporary buffs divided out. In DRG
  a beer is a `UStatTemporaryBuff` in the game instance's `TemporaryBuffs` whose
  `ModifiedStats` map names the max-health stat (`GameData.Stats.MaxHealth`); its
  value is multiplicative or additive by the stat's `ValueModificationType`. Perks are
  the player's normal and count as 100; a beer's extra shows as over 100, blue.
- **Shields vary by build and some missions disable them**: send the current maximum
  with every write (`shield 12/30`), never a constant. A zero maximum means no shield
  drawn, which is right.
- UE4SS here has hot reload off; a changed mod loads on the next game launch, and
  UE4SS truncates its log at each launch, so a watch that counts old lines fires on
  the truncation.

## The game cannot see the host home directory (2026-10-26)

The meter went silent after "an update". UE4SS.log showed the reads still working on the
2026-10-22 game build and `[G13Health] no HOME or XDG_STATE_HOME in the environment`: the
mod built its feed path from `HOME`, and Wine no longer imports it. `/proc/<pid>/environ`
of the game still had `HOME` (pressure-vessel passes it); the drop is Wine's own import
into the Windows environment block, `is_special_env_var` in `dlls/ntdll/unix/env.c`, which
now lists `HOME=`, `TMPDIR=` and the whole `XDG_` prefix. Measured in scratch prefixes with
`wine cmd /c set`: GE-Proton 11-11 and wine-staging 11.20 both drop `HOME`, `XDG_*` and
`STEAM_COMPAT_*`; an arbitrary variable (`G13_PROBE`) and `SteamAppId` pass through.
The pacman log brackets it: the mod worked on 2026-10-07 under GE-Proton 11-7 (installed
09-18); 11-9 arrived 10-18 and 11-11 on 10-23, both "Wine bleeding-edge" rebases. No
cached package was left to bisect between them. Wrong lead on the way: the game's own
update (buildid 26107973, 10-22) and the `S:` library drive UE4SS logs are unrelated.

Fix: the mod reads `feed.txt` beside itself (`debug.getinfo(1, "S").source` gives the
script path; the sidecar sits at `Scripts\..\feed.txt`), written by `contrib/drg-health/
install.sh` from the host path; `G13MAP_FEED` in the environment overrides it, since plain
variables do pass through a Steam launch option. After those the mod takes the host home
from `HOME` (older Wine), `WINE_HOST_HOME` (a Unix path; GE-Proton 11-11 sets it) or
`WINEHOMEDIR` (an NT path: `\??\Z:\home\me` here, `\??\unix\home\me` on older builds, and
the Terraria feeder's fallback only matches the older form, so on this Wine it falls
through; left as a finding, not changed). Don't guess at `USERPROFILE`: Proton's is
`C:\users\steamuser`, not the host home.

Installer test record (asked 2026-10-18): `contrib/drg-health/check.sh` on 2026-10-26,
PASS, covering default Steam discovery under a private HOME, a custom `--mods-dir`,
`--feed` with a space and `--drive Y:`, `mods.txt`/`mods.json`/`enabled.txt` handling,
backup on reinstall, rejections, and the mod's path resolution under `lua5.4` with the
UE4SS globals stubbed (`G13MAP_FEED` > `feed.txt` > `HOME` > none, plus one feed write).
Two mutants failed it as intended: the sidecar name misspelled in the Lua (resolved to
`HOME`), and the installer writing forward slashes. Real install the same day: backup at
`ue4ss/G13Health.backups/20261027T064029Z-263596`, `feed.txt` = `Z:\home\user\.local\state\g13map\health`,
lists unchanged. Live confirmation is the next launch's `loaded; feed ... (feed.txt)` line.
