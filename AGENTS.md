# Making the health meter work for a game

Notes for whoever adds a game to the G13 health meter, assistant or human. The user
side is in [docs/health-meter.md](docs/health-meter.md); this is what it took to get
two games on it, written down so the next one is shorter.

## The contract is one line in one file

A feeder writes `HEALTH[/MAX] [shield S[/MAX]] [helmet on|off] [ttl SECONDS]`, or
`wait`, to `$XDG_RUNTIME_DIR/g13map-g13-0.health` or `~/.local/state/g13map/health`.
`g13map watch` reads both ten times a second and the newest wins. That is the whole
interface: no socket, no protocol, nothing to link against. A shell loop can feed it.

- Always send `ttl` from a feeder that can die with the game; the watcher then ends the
  meter by itself. A feeder that only speaks on changes should still rewrite every few
  seconds to keep the ttl alive.
- Rewriting the file in place is fine (the watcher keeps the last state through half a
  second of empty reads), but write atomically if you can.
- Send `0` when the player is down or dead. The meter holds three dark seconds on any
  drop to zero before showing what comes next, so a death reads as a death even if the
  feed moves on at once. A feeder that sees "alive" one moment and "no player" the next
  should write a `0` first and keep it there for a few hundred milliseconds.
- `wait` means connected but nothing to show: a menu, a lobby, a loading screen.
  Whether a lobby should read `wait` or `100` is a matter of taste; the CS2 listener
  sends `100` in the menu because it looks better on the glass.

## Where a feeder can write

A game inside Steam's container (pressure-vessel, every Proton game) sees a **private**
`/run/user/UID`. Only a few sockets are bound in from the host, so a write there lands
in a tmpfs nobody else can see, while the mod reports success. The home directory is
shared, which is why the second feed path exists. From a Windows game under Proton the
host path is `Z:` plus the Unix path with backslashes; `HOME` and `XDG_RUNTIME_DIR` are
in the Wine process environment. To see what a container sees, look through
`/proc/<pid>/root/` and `/proc/<pid>/mountinfo` of the game process (same user, no root
needed). The kernel truncates the process name to 15 characters, so grep the comm
against a prefix, not the full name.

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

## Games that talk on their own (Counter-Strike 2)

CS2's Game State Integration posts JSON to a local HTTP listener named in a cfg file in
`game/csgo/cfg/`; `g13map health cs2-config` prints it. A profile whose picture is
`health cs2` makes the watcher run the listener itself while the profile is active, so
nothing has to be started by hand. `player.state.health`, `armor` and `helmet` are the
fields; while dead, `player` is whoever is being spectated (compare `player.steamid`
with `provider.steamid`), and the menu has no `player.state` at all. Counter-Strike:
Source has the same armour and helmet but no Game State Integration; it needs another
route (a server plugin), and a profile set to `health cs2` there waits forever.

## Choosing where it shows

The meter is a profile's picture (`g13map profile lcd NAME health`), so the i3 window
rules put it on the game's windows and nowhere else. Two profiles with the same reader
hand the meter across a switch; any other picture ends it, and the ending profile's
own backlight comes back (the meter was once restoring the colour of the profile it
started under, over the new one: if a colour "leaks" between profiles, look there).

## Iterating without the package cycle

- `~/.config/g13map/meter` holds the look (colours per band, the dark hold, beat spacing,
  the flash, the CS2 port) and is re-read within two seconds while the meter runs.
- `tools/dev-watch.sh` runs `g13map-watch.service` from a warm `cargo build --release`
  through a systemd drop-in; `off` restores the installed package. Build the package only
  for what stays.
- `tools/fakedaemon.pl FIFO LOG` stands in for the daemon's pipe reader and logs every
  frame and command it gets; with `G13MAP_PIPE`, `G13MAP_OUT_PIPE`, `G13MAP_CONFIG` and
  `XDG_RUNTIME_DIR` pointed at a scratch directory, `g13map watch` plus `g13map health`
  exercise the whole path without hardware.
- `G13MAP_METER_DUMP=DIR cargo test --lib meter::dump -- --ignored` writes frames of
  every state as PBM files; `magick montage` makes a contact sheet to look at before
  anything reaches the glass.
- Log the feed while playing (a loop that records the file on change with a timestamp):
  it tells whether an oddity is the game, the feeder or the panel.
