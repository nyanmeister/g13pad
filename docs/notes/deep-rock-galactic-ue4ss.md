# Deep rock galactic ue4ss

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Deep Rock Galactic with UE4SS, 2026-10-07 (game version not recorded).
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
