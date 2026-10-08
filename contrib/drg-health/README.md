# Deep Rock Galactic feeder

`G13Health` is a [UE4SS](https://github.com/UE4SS-RE/RE-UE4SS) Lua mod that feeds the
g13pad health meter (`docs/health-meter.md`) with the dwarf's health and shield. Copy
the `G13Health` folder into the game's `FSD/Binaries/Win64/ue4ss/Mods/` and enable it
in `mods.txt` (`G13Health : 1`) or `mods.json`. It reads the pawn's `HealthComponent`
five times a second and writes the feed file `~/.local/state/g13map/health` on the host
through Proton's `Z:` drive (the container shares the home directory, not `/run/user`):

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
