# Doom feeder (Zandronum, GZDoom)

`g13health.acs` is an ACS script for ZDoom-family engines that prints the player's
health and armour to the console, in dark grey, only when one of them changes (asked
2026-10-08: the console is sometimes worth reading, so the line stays rare and quiet;
the watcher keeps the meter alive between lines by itself):

```
G13HEALTH <health> <spawnhealth> <armor>
```

With the engine's console going to a logfile, `g13map watch` follows that file for a
profile in health mode `log` (the path is `log_file` in `~/.config/g13map/meter`,
`~/.local/state/g13map/game.log` by default) and feeds the meter: health over spawn
health (200 after a soulsphere reads as an overshield, blue), armour as the shield
(green armour fills the bar, blue is clamped), 0 when dead.

## Build

```sh
acc -i /path/to/acc g13health.acs G13HLTH.o     # acc: github.com/rheit/acc, needs zcommon.acs
mkdir -p pk3/acs && cp G13HLTH.o pk3/acs/ && cp LOADACS pk3/loadacs.txt
(cd pk3 && bsdtar --format zip -cf ../g13health.pk3 acs loadacs.txt)
```

`bsdtar -a` picks the format from the extension and `.pk3` means nothing to it, so it
writes a tar: the engine lists the file with no lumps and nothing runs. `--format zip`
is the fix; `zip -r` works too. The built `g13health.pk3` is committed beside the source.

## Run

Load the PK3 with every game and send the console to the logfile with a fixed name:

```sh
zandronum ... -file ~/.local/share/g13map/g13health.pk3 \
    +sv_logfilenametimestamp false +logfile ~/.local/state/g13map/game.log
```

GZDoom takes the same two options without the `sv_` one (it never adds a timestamp):

```sh
gzdoom ... -file ~/.local/share/g13map/g13health.pk3 +logfile ~/.local/state/g13map/game.log
```

Zandronum appends a timestamp to the log's name unless `sv_logfilenametimestamp` is
off; the follower copes either way (it takes the newest file whose name starts with
`log_file`), but the fixed name keeps the directory tidy. Put the two options in your
launcher script. Then a profile in health mode `log` (`g13map profile health doom log`) and a window rule for the engine's
class (`zandronum`, `gzdoom`) do the rest.

The script starts with `#library "G13HLTH"`: without it a library's string constants are
looked up in another mod's string table, and with Brutal Doom loaded the tagged line
came out as `GoFatality87BDWeaponAction100BDWeaponAction0` (the numbers right, the
words someone else's). Zandronum 3.2 answers 0 for `APROP_SpawnHealth`; the script
takes 100 then. GZDoom
can also print its console to stdout with `-stdout`, but the logfile route is the same
for both engines.
