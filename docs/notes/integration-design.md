# Integrations: one design question

Written 2026-10-13 by Claude (Fable 5.1). This is the open design question behind the
project AGENTS.md's "the watcher knows a feed grammar, not games"; it is here so the
next person adding a game sees the choice before adding a field to `Extra`.

## Stipulation (asked 2026-10-13)

Integrations are read-only, so a reader must *attempt* newer game versions, never refuse
them. "Verified against X" is a record, not a gate. If a newer version defeats the reader,
the honest failure is an empty/unavailable display and the user turns the LCD to another
mode. Readers already validate what they read (pointer bounds, finite numbers, bounded
collections, ten-bad-snapshots-then-disable), which is what makes attempting safe.

## Installation (asked 2026-10-18)

Every future program integration must ship a basic installation script. Make relevant
paths and settings configurable through documented options or configuration, rather than
hard-coding this machine's setup. Test the actual installer with its defaults and with
at least one non-default configuration in an isolated destination/session. Record the
commands and results in that integration's note before calling its setup complete.

## Inventory: where each game's knowledge lives today (0.2.89)

| Game | Reader process | Record on the feed | Watcher-side knowledge |
|---|---|---|---|
| Doom, DRG, CS2, GoldSource, Source, ULTRAKILL | own feeder (contrib/, ACS/Lua/C/C#) | health line + `Extra` words | `Extra` fields + fixed positions in render.rs |
| Terraria | own feeder (contrib/, C#) | health line + mana/defense/breath | same |
| Factorio | own process, `g13map-factorio` (src/factorio_main.rs, 949 lines, in this crate) | health line + vehicle/pilot/battery/research/attack | same, plus gear/crack art |
| Dwarf Fortress fort / adventure / travel | DFHack Lua (contrib/df-health) | `fort 2` / `adv 1` / `travel 1` positional records | fortress.rs 725, adventure.rs 334, travel.rs 229: parsers by array index + bespoke screens |
| RetroArch BN2, Dolphin F-Zero GX | **in the watcher** (src/ra/, 3.5k lines: memory transport, log transport, adapters, two renderers) | none — reads shared RAM / log directly | everything |
| mpv, pressure, audio, MPD | in the watcher (system tools, not games) | — | — |

So "move the readers out" is mostly done already; the feed contract did its job. What
remains in the watcher is (a) the one in-process game reader, `ra/`, and (b) per-game
*presentation*: a 16-field `Extra` bag drawn at hard-coded positions, three DF parsers
reading fields by position, and two emulator screens.

## The 2026-10-12 review's five findings are one question

1. feed.rs `None` conflation — fixed (0.2.87, `Outcome::Off | Live`).
2. df-health diagnostics save/restore globals — local, Lua; leave to the DF pass.
3. `Extra`: three games in one bag; drawing order is layout policy.
4. `pub(super)` 185 / `use super::*` 23 across meter/ and ra/.
5. factorio_main.rs:18 / adventure.rs:61: named concepts as array positions.
6. ra/adapters.rs: registry dispatch duplicated per game.

3, 5 and 6 are the same thing from three sides: **the watcher knows each game by name.**
Every new game adds fields to `Extra`, a branch in render.rs, a positional record parser,
or a registry row. 4 is the coupling that lets that happen cheaply.

The question: should the watcher know games at all, or only a palette of elements?

## Options

**A. Element palette.** The feed grammar grows a small set of generic elements a feeder
may send, and the watcher draws elements, not games:
`gauge NAME CUR/MAX`, `count NAME N`, `clock S`, `rank NAME I/N`, `flag NAME on|off`,
`text NAME "..."` (marquee when long), `table ...` for the fortress overview,
`icon NAME` for a named sprite (`v1`, `heart`, `gears`) from the existing tuning grammar.
Layout is a slot order, not a per-field coordinate: the renderer fills slots in the order
the feeder sent them, with the health bar and heart fixed. `Extra` becomes
`Vec<Element>` (bounded, say 8). The DF records become element lists too, so
fortress.rs/adventure.rs/travel.rs shrink to the pieces that are genuinely bespoke
(the season backlight, the urgency bands) or vanish into the palette.
- Wins: a new game is a feeder plus zero watcher changes; findings 3, 5, 6 dissolve; the
  watcher's per-game test surface goes away; the feed line stays human-readable.
- Costs: the fortress overview and F-Zero gauge are *designed* screens; a slot order
  cannot reproduce them exactly. Keep a named-screen escape hatch (`screen fortress`)
  for the two or three that earn it, and accept that most games get the palette look.
- Risk: the palette grows into a second GUI toolkit. Cap it at the elements above; a
  feeder that needs more sends a sprite.

**B. Keep bespoke screens, move only `ra/` out.** `g13map-ra` becomes a feeder process
(the launcher already owns RetroArch's pipes; the Dolphin log reader is a file follower)
writing a record the watcher renders as today. findings 3/5/6 stay; coupling stays.
- Wins: watcher loses 3.5k lines of transport and one hardware dependency; cheap.
- Costs: adds one more positional record type (the thing 5 complains about) unless the
  record is already palette-shaped — which argues for doing A first, or at least
  designing the `ra` record in A's grammar.

**C. Status quo**, add games as before. Each adds ~300–900 watcher lines and a renderer
branch; the AGENTS.md paragraph per game is the documentation of that cost.

## Lean

A, 70%, with B as its first step done in A's grammar: move `ra/` out as `g13map-ra`
emitting palette elements (`gauge speed`, `gauge energy`, `flag boost`, `rank style`),
and let the watcher's F-Zero screen be the test of whether the palette look is good
enough on the glass. If it is not, that is the moment to decide the named-screen
escape hatch, with a real screen to compare against rather than a guess.

Reasons for the lean, in order of weight: the feed line already proved that one grammar
can carry nine games; the two largest watcher areas (meter 3.6k, ra 3.5k) are the
per-game presentation this removes; and every lesson in the project AGENTS.md about a
game is a lesson about the *feeder*, which is where the game-shaped code belongs.

Where I could be wrong: the designed screens are a large part of why the owner likes
this. If the palette look reads as "generic" beside the fortress overview, option A
costs the project its charm, and B with a tidy `ra` record is the right size.

## Stipulation, first application

`src/ra/fzero.rs` binds to the disc id `GFZE01` **and revision 0** (`\0\0`); the test at
~693 writes revision 1 and expects rejection. Under the stipulation this should accept
any `GFZE01` revision (and arguably any region's F-Zero GX), attempt the read, and let
the existing pointer/bounds/finite validation decide. A revision whose layout differs
fails validation and shows the dashed empty gauge the code already has for unsupported
physics. Factorio is the model: `BUILD_ID` mismatch is logged and the last layout is
tried (factorio_main.rs ~222). Terraria's sha256 check is a stale-copy guard for the
patched executable, not a version gate; keep it.

## Plan for item 3 (trial)

1. Define the element palette grammar in docs/health-meter.md (feed side) with the
   bounded parser in src/meter/feed.rs and a shrink-pinned fuzz case; `Extra` → elements
   behind the same `Outcome`.
2. `g13map-ra`: move src/ra/{transport,core_memory,adapters,runtime}.rs into a bin that
   writes elements; the watcher keeps only `Reader::Feed`. Keep `g13map ra launch` as the
   user-facing name (it execs the sibling, like `edit`).
3. Compare on the glass: F-Zero through the palette vs the current fzero.rs screen,
   frames dumped side by side (`G13MAP_METER_DUMP`). The owner judges on the glass.
4. If kept: DF records next; fortress.rs becomes `table` + season backlight.
5. If not kept: revert to B's shape — `g13map-ra` stays out of process with its own
   record, and the named screens stay.

## Revision, 2026-10-13: option A withdrawn

The owner, on a side-by-side of shipped screens against a palette mock: the F-Zero
screen is right as it is, because it shows what the game does not (the theoretical
maximum speed, the share of it as a pie, the achievement progress); the palette column
only repeated what is already on the monitor. **The principle: the best integration
reveals stats the game does not expose.** Health mode earns its place the same way, as a
peripheral-vision read (the colour, the beat), most valuable in GoldSource.

So the question became *what the watcher owns*, and the answer is a kit of composable
parts (`meter/parts.rs`: the heartbeat with its hold, the pie; with the gauges, bars,
glyphs and sprites beside them in `render.rs`) that bespoke per-game screens compose,
with the game's knowledge and its derived stats in the feeder or adapter. The
2026-10-12 review's findings 3, 5 and 6 are answered by the kit and by keeping every
per-game word inside `Meter`'s two `extras_*` hooks, not by a grammar. First step done
in 0.2.91: the heartbeat, the hold and the pie out of `Meter` and F-Zero, every exported
frame byte-identical before and after.


## Revision, 2026-10-13 (later): three generic words

Decided by the owner with Claude (Opus 4.8) for the Ship of Harkinian reader and shipped
in 0.2.97: `deaths N`, `count LABEL VALUE[/MAX]` and `foe LABEL CUR/MAX`. The promotion
test for a *named* word is that it recurs across games and wants one presentation (a
death tally; the lock-on enemy bar). Anything else is a `count` the feeder labels. This
is not option A returning: three bounded words, no game named, and the designed screens
stay. Deferred until a second game asks: score, lives, ammo, kills, floor.
