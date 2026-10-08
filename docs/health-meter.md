# Health meter

The watcher (`g13map-watch.service`) can turn the LCD and the backlight into a health
meter for the game in front of you: a monitor trace that beats faster as health falls, a
bar with notches at the band edges, a readout, a hatched shield bar and a ring around the
heart while there is any shield or armour (a ring per hundred of it, up to three, and a thicker bar past 100, as
with Doom's blue armour), and the backlight by band. The default ladder, every edge and
colour of which is a line in the tuning file (below):

| health | backlight |
|---|---|
| over 100 | blue (an overshield) |
| 76–100 | green |
| 51–75 | yellow |
| 26–50 | orange |
| 1–25 | red; every beat flashes the panel |
| 0 | off and a flatline for three seconds (always, whatever the feed says next), then the search for a pulse (below) under red while it stays 0 |

While a game is connected but has no health to report (a lobby, the menu, a spectator
seat) the panel shows a monitor sweep searching for a pulse and the backlight keeps the
profile's colour.

## Choosing it per profile

A profile has a **Health mode** tick in the editor's LCD section (or `g13map profile
health deeprock feed`), so the window rules put the meter on the game's windows and
nowhere else; the profile's own picture stays kept and shows again when the tick comes
off. The mode names the reader:

- `feed`: something else writes the feed (a game mod, a script, `g13map health`).
  Without a feed the panel waits, searching for a pulse.
- `cs2`: the watcher runs the Counter-Strike 2 Game State listener itself while the
  profile is active (port `cs2_port` in the tuning file, 3000 by default), so nothing
  has to be started by hand; the game's cfg must still be in place.
- `log`: the watcher follows a game's console log (`log_file` in the tuning file).

Two profiles with the same reader hand the meter across a switch without a restart; a
profile with another picture ends it. `g13map health demo` only shows while the active
profile has the meter.

## The feed

Any program can feed the meter by writing one line to
`$XDG_RUNTIME_DIR/g13map-g13-0.health` (the suffix follows the daemon's pipe name) or to
`~/.local/state/g13map/health` (`$XDG_STATE_HOME`); the newest file wins. A game inside
Steam's container has a private `/run/user` and shares only the home directory, so a
mod in there uses the second:

```
wait                               connected, no health yet
HEALTH[/MAX] [shield S[/MAX]] [helmet on|off]
                                   health (over 100% is an overshield), the shield, and
                                   whether the head is covered (the shield bar is solid)
... ttl SECONDS                    the line expires unless rewritten in time
off                                no game (removing the file does the same)
```

`g13map health ...` writes the file with the same words, so a shell script can feed it:
`g13map health 87 shield 50`, `g13map health wait`, `g13map health off`. Write the file
atomically (beside, then rename) if you write it yourself. Give a feeder that could die
with the game a `ttl`, so its meter dies with it; a feeder that only speaks on changes
needs no `ttl`. The watcher reads the feed ten times a second and renders the meter
itself, one frame per 100 ms, the daemon's reading pace.

`g13map health demo` runs through every state in about forty seconds, for a look at the
meter without a game.

## Counter-Strike 2

CS2 posts its game state to a local HTTP listener (Game State Integration). Put the
configuration where the game reads it, then run the listener while you play:

```sh
g13map health cs2-config > "$HOME/.local/share/Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg/gamestate_integration_g13map.cfg"
g13map health cs2        # listens on 127.0.0.1:3000 (give both a port to change it)
```

The game sends `player.state.health`, `armor` and `helmet` (a solid shield bar). In the menu or a lobby there is no
state, and the meter shows full health with no armour. Dead, `player` is whoever you
spectate: the meter drops to zero (three dark seconds) and then waits until the next
round. Every line the listener writes carries a 30-second `ttl`; the game
posts a heartbeat every 5 s, so a closed game is a meter gone within half a minute.

## Deep Rock Galactic

`contrib/drg-health/G13Health` is a UE4SS Lua mod that reads the dwarf's `HealthComponent`
five times a second and writes `~/.local/state/g13map/health` through Proton's `Z:` drive
(the container shares the home directory, not `/run/user`): health over the
dwarf's normal maximum (`MaxHealth` with any beer divided out, so perks count as 100%
and a Red Rock Blaster's extra shows as blue), the shield as DRG's "armor", 0 when down, `wait`
without a local dwarf; 15 s `ttl`, rewritten every 5 s. Its README has the install.

## Doom (Zandronum, GZDoom)

`contrib/doom-health` is an ACS script that prints `G13HEALTH health spawnhealth armor`
to the console on every change; with the engine's console sent to a logfile
(`+logfile PATH`, `+sv_logfilenametimestamp false`) a profile in health mode `log` makes the
watcher follow that file (`log_file` in the tuning file) into the meter: a
soulsphere's 200 reads as an overshield, armour is the shield, 0 when dead. Its README
has the build and the launcher options.

## Source engine games (Half-Life 2, Counter-Strike: Source, ...)

The Source 2013 client has a Logitech LCD feature of its own: with `-g15` in the launch
options it loads `bin/g15.so` and renders `resource/g15.res` into it four times a second.
`contrib/source-health` is that module, feeding `~/.local/state/g13map/health` with the
player's health, `wait` without a live player and `0` at a death.
`g13map health source DIR` puts the module (of the client's architecture) and the page
into the game's folder; then add `-g15` in Steam and give the game's profile health mode
`feed`. Two modules: multiplayer games (VAC-secured) get the plain feeder and health
only; singleplayer games (Half-Life 2 and its episodes) get one that also hooks the
client's `Battery` user message for the shield bar, since armour is on no page token.
The installer picks by the game's `gameinfo.txt`; its README has the details.

## Other games

A feeder is anything that learns the health and writes the line. A feeder that rewrites
its file in place is fine: the watcher keeps the meter through half a second of empty
reads. Zandronum and GZDoom can log an ACS script's `Log()`
output to a console logfile, which a few lines of shell can follow into `g13map health`.

## Tuning without a rebuild

The meter's look lives in `~/.config/g13map/meter`, written with its defaults the first
time the watcher runs and re-read within two seconds of a change while the meter is
on: the backlight ladder, one `band NAME FROM R G B` line per band, lowest first (the band
at 0 is death, the lowest above it the alarm; add `band purple 201 160 0 255` for a game
whose health runs past 200, drop or move the others as you like), `hold` (seconds of dark flatline after a drop to zero), `calm` and `racing` (seconds
from beat to beat at full health and on the last point; 2.0 and 0.8 by default, and a
beat is never shorter than 0.8 s, the width of one complex on the glass), `swell` (seconds
the heart stays big on each beat, 0.4 by default), `flash panel|trace|off` for the alarm
band's flash on each beat, and where the heart (`heart X Y`, its top-left corner) and the
readout (`readout RIGHT TOP`) sit on the 160 by 43 panel. Edit, save, look at the glass.

For changes to the code itself, `tools/dev-watch.sh` runs `g13map-watch.service` from a
warm `cargo build --release` of the checkout through a systemd drop-in (about half a
minute after the first build) and `tools/dev-watch.sh off` puts the installed package
back; the package cycle is for what stays.
