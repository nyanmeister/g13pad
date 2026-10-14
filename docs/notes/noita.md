# Noita

Written 2026-10-13 by Claude (Fable 5.1), from the first integration session (survey,
probe and 0.2.99 in one evening; the private record is the maintenance notes, noita-20261013).

**Verified against:** Noita build "Jan 25 2025" (PE timestamp `0x6794ee3c`, Steam
buildid 17130612) under GE-Proton11-7, 2026-10-13.
**Newer versions:** attempted, never refused (asked 2026-10-13): an unknown PE timestamp
goes through the engine reader's own discovery pass over the executable image, logged as
such; readings are validated (finite, bounded) and a failed read shows `wait`.

## No mod: achievements are the constraint

Noita disables Steam/GOG achievements, run records, win streaks, Bones files, the gods'
messages and the Cauldron Room for *any* enabled non-translation mod (wiki: Mod
Restrictions). The owner wants achievements, so `g13map-noita` is a host-side memory
reader, not a mod: necauqua's `noita-engine-reader` (MIT, pinned by rev in Cargo.toml)
knows the entity manager, tag buckets and component layouts per build and reads the
32-bit `noita.exe` through `process_vm_readv`. The game never sees it.

## Launch it as the game's parent

`g13map-noita %command%` as the Steam launch option, like the Factorio feeder. The game
is then a descendant of the feeder, which is what Yama's default `ptrace_scope=1` allows
without any capability. `--observe` reads an already running game instead and needs
CAP_SYS_PTRACE: for development, `~/.local/bin/caprun g13map-noita --observe` on this
machine (`caprun` carries the capability as a file cap and hands it on as an ambient
one; source `~/.local/src/caprun`, outside this tree).

## What the feed carries

`HP/MAX ttl 3 deaths N count MANA cur/max count RLD frames deck AT NEXT ICON...`

- Health is `DamageModelComponent.hp * 25` (the game stores a twenty-fifth); `0` when
  it reaches zero, `wait` with no player entity (menu, loading). A polymorphed player
  keeps its health line (the tag `polymorphed_player`) but no wand words.
- `deaths` is the lifetime death count from the global stats block; the per-session
  block only updates at run end, so there is no live kill count yet.
- The held wand is `Inventory2Component.m_active_item` matched against the children of
  the `inventory_quick` entity; its `AbilityComponent` gives mana and the recharge
  (the later of `m_reload_next_frame_usable` and `m_next_frame_usable` minus the
  game's frame counter; `m_reload_frames_left` is the recharge's length, not a
  countdown: it read a constant 28 through six minutes of play).
- The deck: the wand's child entities with an `ItemActionComponent`, in
  `ItemComponent.inventory_slot.x` order. The deck *pointer* lives in the game's Lua
  gun script and cannot be read, so it is reconstructed: each increment of the wand's
  `stat_times_player_has_shot` advances it by one cast under the gun script's rule
  (a card consumes a draw and adds its own: modifiers `draw_actions( 1 )`, multicasts 2
  or 3, triggers their payload count), a recharge (`m_reload_next_frame_usable` still ahead of the frame counter) or a
  wand change resets it. The rule
  numbers come from `data/scripts/gun/gun_actions.lua` inside the game's own
  `data.wak` (a flat index: `[0][count][size][0]` then `offset size namelen name`
  entries), as do the 16x16 spell icons (`sprite = "data/ui_gfx/gun_actions/X.png"`,
  thresholded to one bit: opaque and brighter than 70/255). The strip keeps slot order and the
  frame walks (asked 2026-10-13: a frame that stays on a rotated strip says nothing
  with two identical bolts). Shuffle wands send `next` 0 (no frame): the order is
  random by design.
- Not yet read, all reachable: status effects with frames left (child entities with a
  `GameEffectComponent`), levitation seconds (`CharacterDataComponent`, see below),
  damage multipliers, the nearest enemy for `foe`, biome and depth, orbs this run.

## Layout gotcha: the docs' order is not the memory order

`tools_modding/component_documentation.txt` (CRLF) lists members, then "Custom data",
then privates. `Inventory2Component` is laid out exactly so. `CharacterDataComponent`
is not: twelve undocumented words of collision data come first, the documented run
starts at word 13 (`mass`), and `mFlyingTimeLeft` is data word 50 (+0xc8) with its
maximum at word 17, verified against the HUD bar (30 samples, mean error 0.019). Dump
the words (`noita-probe dump`, `~/.local/src/noita-probe`) and anchor on documented
defaults (liquid coefficient 9, recharge removal 8) before declaring a struct.

## The screen

The owner picked "wand first" from four real-meter mocks (private maintenance notes, mockup-20261013):
the deck strip of real spell icons in the top-middle slot with the next cast framed,
mana and recharge in the band, deaths at the right. The `deck` word is generic (a
weapon rack would ride it too); its renderer is pinned to the picked mock pixel for
pixel by `deck_strip_matches_the_picked_mock`. The kit's font is uppercase only.
