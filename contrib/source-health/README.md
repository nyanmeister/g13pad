# Source engine feeder (Half-Life 2, Counter-Strike: Source, ...)

Valve's Source 2013 client has a Logitech G15 LCD feature built in, on Linux too, that
nobody ever shipped a Linux module for. Started with `-g15`, the game loads `bin/g15.so`
through its own `CreateInterface`, reads `resource/g15.res` from the mod's search path,
and every 250 ms renders the page's text items, with `%(localplayer)FIELD%` tokens filled
from the player's prediction data, into the module's `SetText`. `g15.cpp` is that module:
it turns the rendered line into the health meter's feed file,
`~/.local/state/g13map/health`.

Verified 2026-10-08 on Counter-Strike: Source (64-bit, Steam's sniper container) and
Half-Life 2 (32-bit): health live, 0 at death, `wait` on the menu, the team menu and the
spectator seat. Any game on that branch should do the same (Team Fortress 2, Day of
Defeat: Source, Half-Life 2: Deathmatch, the episodes); Portal 2 has the hook as well.

## Singleplayer and multiplayer

Two modules come out of the same source, and the installer picks by the game's own
label (the `type` key in its `gameinfo.txt`; `sp` or `mp` after the folder overrides):

- **`g15-mp`** (multiplayer, and any game that does not say): the feeder alone. It
  touches nothing of the game's and reads only what the engine hands it. Health only.
- **`g15-sp`** (singleplayer: Half-Life 2 and its episodes, Lost Coast): the feeder plus
  `armour.cpp`, which finds the client library's `CreateInterface`, takes its
  `VClient017` interface and patches the vtable slot of `DispatchUserMessage` so every
  `Battery` user message (the only place the client ever sees armour) also feeds the
  shield bar. That is a code patch inside the game's process, the thing anti-cheat
  looks for, so it is never installed into a multiplayer game by default. The numbers
  (interface version, slot 36, message 15) are the 2013 branch's; another branch needs
  its own and the module says "armour hook: ..." in the hunt log when they do not fit.

## Install

```sh
g13map health source "$HOME/.local/share/Steam/steamapps/common/Half-Life 2"
```

That copies the module of the game's kind and the client's architecture
(`g15-sp-i386.so`, `g15-mp-x86_64.so`, ... from `/usr/lib/g13pad/source-health/`) to
where the engine looks, `bin/g15.so` or `bin/linux64/bin/g15.so`, and writes `g15.res`
into every mod folder's `custom/g13pad/`.
Then, by hand:

1. Steam → the game → Properties → Launch options: add `-g15`.
2. A profile in health mode `feed` with a window rule for the game
   (`g13map profile health hl2 feed`; the window class is `hl2_linux`,
   `cstrike_linux64`, ...).

`g13map health source DIR remove` takes it out again. `g13map health source-res` prints
the page file for a game set up by hand.

The console says `Logitech LCD Keyboard initialized` when the module loaded. If `-g15`
is set and that line is missing, the module was not found (wrong folder, or the game is
the other architecture) or failed to load.

## What the games give

The page asks for `m_iHealth` and `m_lifeState`; both are on every Source 2013 player.
Counter-Strike: Source reports health 1 for a dead or not yet spawned player and
Half-Life 2 reports 0; the life state tells them apart from one point of health, so the
feed says `wait` before a spawn and `0` (then `wait`) after a death.

**Armour is not on the page's channel** (2026-10-08): the client player has no armour
member at all in this branch (the server sends a `Battery` user message and only the
battery HUD element keeps it), and neither HUD publishes an armour global stat (HL2's
publishes `(ammo_primary)`, `(ammo_secondary)`, `(weapon_name)`, `(weapon_print_name)`,
`(mapname)`, `(time_int)`; CS:S's only the last two). Hence the singleplayer module's
hook above; the multiplayer module leaves the shield bar empty. If a game does expose
armour as a field, add the token as a fourth word of the line
(`G13 %(localplayer)m_iHealth% 100 %(localplayer)m_ArmorValue% %(localplayer)m_lifeState%`)
and either module takes it as the shield, ahead of the hook's.

## Hunting a field

To see a game's fields, run it with `-g15 -condebug` and type `g15_dumpplayer` in its
console: the list lands in the mod folder's `console.log`. Only top-level names work;
`a.b` names hit an assertion in the engine's lookup and render empty.

To watch them during play, `tools/g15-probe-res.sh console.log` turns that dump into a
page that asks for every top-level field and every global stat (the health line stays
first, so the meter keeps working), to be written over the mod's
`custom/g13pad/resource/g15.res`. Then:

```sh
touch ~/.local/state/g13map/g15.log     # the module logs while this file exists
```

Every line the engine renders is appended there as it changes, control bytes as
`\xNN`. Play, pick things up, and `grep -v 'G13 ' ~/.local/state/g13map/g15.log` shows
what moved. Remove the file to stop; `g13map health source DIR` puts the plain page back.

## Caveats

- The module is loaded by the game itself through the hook Valve built for Logitech's
  own DLL, and a third-party replacement DLL has existed on Windows for years. No VAC
  action against it is known; none is promised either.
- Built without libstdc++ and without glibc symbols newer than 2.4, so it loads inside
  Steam's runtimes; `tools/check-g15-symbols.sh` fails the build's tests otherwise.
- Half-Life 2 ignores typed console input from xdotool but takes F-keys: for scripted
  tests, bind commands in a cfg and `+exec` it from the launch line.
