# Deep Rock Galactic feeder

`G13Health` is a [UE4SS](https://github.com/UE4SS-RE/RE-UE4SS) Lua mod that feeds the
g13pad health meter (`docs/health-meter.md`) with the dwarf's health and shield.

## Install

With UE4SS already in the game (`FSD/Binaries/Win64/ue4ss/`), close the game and run:

```sh
./install.sh
```

With no `--game-dir`, it checks the standard Steam paths under HOME. It copies the
`G13Health` folder into the game's `ue4ss/Mods/` (a previous copy is backed up beside
`Mods`), writes `feed.txt` in the mod folder with the host feed path as the game sees it
(`Z:\home\you\.local\state\g13map\health`), and, when neither `mods.txt` nor `mods.json`
names the mod, enables it with an `enabled.txt`. `--mods-dir`, `--feed` and `--drive`
cover other layouts (`install.sh --help`); `check.sh` tests the installer and the mod's
path resolution privately. The mod loads on the next game launch; `UE4SS.log` then
carries `[G13Health] loaded; feed Z:... (feed.txt)`.

The sidecar exists because the game cannot find the host home directory itself: Wine
drops `HOME` and every `XDG_*` variable from the Windows environment (since GE-Proton
11-9; 11-7 still passed `HOME`), and the mod's old `~/.local/state` guess went silent
with "no HOME or XDG_STATE_HOME" in the log. The resolution order is `config.feed` in the
script, a `G13MAP_FEED` environment variable (arbitrary variables do pass through, so a
Steam launch option `G13MAP_FEED='Z:\...' %command%` works without the installer),
`feed.txt`, then the host home from `HOME`, Wine's own `WINE_HOST_HOME` or `WINEHOMEDIR`
(current builds still set those, so the mod usually recovers even without `feed.txt`;
the sidecar makes the path explicit and survives the next Wine change).

## What it writes

It reads the pawn's `HealthComponent` five times a second and writes the feed file
`~/.local/state/g13map/health` on the host through Proton's `Z:` drive (the container
shares the home directory, not `/run/user`):

```
HEALTH/BASE shield ARMOR/MAXARMOR ttl 15     a local dwarf (0 health when down)
wait ttl 15                                  no local dwarf: menu, loading, spectating
```

`BASE`, the 100% point, is the dwarf's `MaxHealth` as the game reports it with any beer
divided out (a beer is a temporary buff in the game instance's list that modifies the
max-health stat; the Red Rock Blaster multiplies it by 1.3 for a mission). Perks that
raise the maximum are part of the dwarf and count as 100%; the beer's extra shows as
over 100% (blue). The Space Rig pawn reports the maximum without perks, the mission
dwarf with them, and each is its own 100%. Set `base_max_health` in the script to fix
the point instead. The line carries a 15 s
`ttl` and is rewritten every 5 s, so a closed game ends the meter within 15 s.

What the mod reads is public, read-only game state; it changes nothing in the game.
Mod tiers on mod.io are a packaging question for whoever publishes it there.
