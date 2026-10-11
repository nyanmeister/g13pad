# Battle Network 2 Style meter

Use an existing RetroArch profile in `retroarch` health mode, and launch with:

```sh
g13map ra launch -L /path/to/mgba_libretro.so /path/to/BN2-USA.gba
```

The shared launcher detects supported games and selects their memory reader.
`g13map ra bn2 launch` remains a compatible alias. The launcher also works without
core/content arguments: load the game through RetroArch's menu. Keep the launcher
running. It uses RetroArch's existing login
and does not change Hardcore. The LCD keeps achievement trophies first, followed
by the equipped Style, next-Style battle progress, current vote leader, equipped
Style upgrade progress, and the RA completion bar. Before the first story Style
event, next-Style progress says `STORY LOCK`. Normal and Hub have no upgrade meter;
V3 says `MAX`. Vote leadership is a tendency: Hub can override the ordinary result.

`g13map ra bn2` reports all four vote totals and counter details. To export a
160×43 preview of the current validated data, use
`g13map ra bn2 preview /tmp/bn2.pgm`.

This adapter supports mGBA and both USA files:

| Build | Reference CRC32 | MD5 |
| --- | --- | --- |
| Retail USA | `6D961F82` | `39f8a42133df444eeb7bf0b2194d6286` |
| No MP Hard Mode v1.1 (Souzooka) | `E99CE5C5` | `76a3ca72300db4cff7bb87873cde2b9c` |

Full-file comparison found only two changed bytes, at ROM offsets `1C712..1C713`
(`12 DB` → `00 00`). Every inspected Style routine and address is identical.
The diagnostic reports which variant is loaded. Other revisions,
title/loading screens, unsupported cores and stale data retain the general RA
overview. Hidden counters work through the shared G13 launcher; unwrapped native
launches retain the general achievement overview through native logging.

## Local transport and settings

**The private pipe must be created when RetroArch starts.** It cannot be attached
to an already-running unwrapped process. This uses the stock executable's existing
command support, without a RetroArch patch or plugin. The pipe is reusable, but
every game needs a verified reader and a core exposing the relevant memory.

The wrapper owns private stdin/stdout pipes. Once BN2 is detected, it emits only fixed
`READ_CORE_MEMORY` requests: ROM header (32 bytes), upgrade routine signature
(8 bytes), variant signature (2 bytes), and three RAM blocks (72, 16 and 1 bytes),
up to five times a second. Unknown games and unloaded content leave the pipe idle.
Switches and same-game reloads invalidate any in-flight collection before it can
publish. Direct uncompressed ROM files can be recognized by bounded file signatures
before RA login completes; RA identity, when available, supersedes that hint. It
checks the USA revision-zero header before and after the reads, and the retail
upgrade thresholds and adjacent code. These are signatures, not a full-ROM hash;
patches beyond the verified No MP Hard Mode v1.1 change are unsupported. No network command server,
API key, debugger, memory writes, achievements inference or guessed battle counts
are needed. Snapshot files live inside the user's private wrapper session folder;
the watcher checks PID, process start, revision and a two-second freshness limit.
Menus can show Style progress; title and loading phases do not.

It deliberately avoids `GET_STATUS`: RetroArch 1.22.2 can crash in that handler
when a core-info record has a null system ID. Missing memory descriptors instead
return an error that the reader safely rejects.

A temporary appended config enables stdin commands and disables automatic config
saving on exit for this launch, preventing the temporary pipe setting from being
saved into the normal config. Existing appended configs are preserved. If you
want permanent RetroArch setting changes, make them during an ordinary launch.
Manually saving this launch's config can persist its temporary settings. The
wrapper never enables network commands or changes the achievement mode.

## Address provenance

Console addresses below are GBA EWRAM. RetroAchievements addresses add `0x8000`
to the EWRAM offset; RA's first `0x8000` bytes are IWRAM. See the official
[rcheevos GBA map](https://github.com/libretro/RetroArch/blob/v1.22.2/deps/rcheevos/src/rcheevos/consoleinfo.c).

| Console address | Meaning / retail ROM routine offset |
| --- | --- |
| `02000DC0`, `02000DC1` | Phase and equipped Style, [RA game 773 CodeNotes](https://retroachievements.org/codenotes.php?g=773) |
| `02000DE4` (u32 LE) | Next Style qualifying battle starts; routine `4E1A` checks story flag `002B`, increments and sets offer flag `002A` at 280 |
| `02000005` | Story flag `002B` mask `10`, offer flag `002A` mask `20`; flag accessor `1BF40` |
| `02003A68/6C/70/74` (u32 LE) | Guts/Custom/Team/Shield persistent votes; result routine `652E`, selector `11612` |
| `02000DA1..DB0` (u8) | Upgrade counters indexed `(category−1)*4+(element−1)`; routine `1142A` |
| `02000DA0` | Upgrade pending flag, set when the counter reaches its requirement |

Upgrade thresholds are Guts 120, Custom 160, Team 130, Shield 100, at both upgrade
steps. V3, Normal and Hub do not increment. The game controls upgrade-counter resets;
new Style offers reset the vote block and next-Style counter (`124E4`). The Style
selector breaks equal totals Shield > Team > Custom > Guts. Per-battle action
scores at `02003A50..67` are separate and reset each fight; they are not the
persistent vote totals. The first story Style event has different timing, so its
progress is not represented as an ordinary 280-battle cycle. Fresh saves leave
the next-Style counter uninitialized (`FFFFFFFF`) until that event; the reader
reports it as unknown while locked, rather than interpreting it as a count.

The [Style guide](https://gamefaqs.gamespot.com/gba/561218-mega-man-battle-network-2/faqs/19851)
describes the mechanics; [the TAS author's notes](https://tasvideos.org/3385S)
identify the vote locations. Retail ROM inspection confirms full 32-bit fields,
thresholds, indexing, story gate, resets and tie order. Runtime reads use the
official [RetroArch command handlers](https://github.com/libretro/RetroArch/blob/v1.22.2/command.c),
with bounded requests and no write commands.
