# Dwarf fortress dfhack

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Dwarf Fortress 53.16 with DFHack 53.16-r2 native Linux, 2026-10-09.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Dwarf Fortress: DFHack is the boundary

`contrib/df-health/g13-lcd.lua` is a read-only DFHack script, feeding a bounded
`fort 2` record into a separate fixed overview (older `fort 1` records still work).
Do not invent player HP for fortress counts. `getCitizens(false, true)` includes living citizens and residents,
including insane citizens; excluding residents made a native population of 112
read 66. Count affected wearers, not worn items; babies are excluded from clothes.
DF 53.16 settings are `d_init.feature.flags.TEMPERATURE/WEATHER`. Missing fields
must produce waiting/error data, never a fabricated all-clear. Inspect actual
wound, syndrome, health-request and limb fields in a copied fortress before relying
on schema alone. Surface samples are observations, not a freeze forecast.

Use a frame timer with elapsed-ms gates: simulation ticks stop while paused,
and repeat-util's world-unload handling cancels registered tasks. Validate pause,
title unload and another save load on the real DFHack boundary. Independent
startup files must start with `dfhack` and end with `.init`; reversed naming never
autoloads. Urgent events compare severity, so healing an infection into a less
urgent care request does not flash again. Preserve the LCD cooldown across reader
threads/profile changes.

The Steam DFHack launcher runs `pidof -s dwarfort` before launching: an isolated
copied game on Xvfb still blocks the user's ordinary Steam launch. Stop our
confirmed private PID and verify exit before inviting a live check. SIGTERM was
ignored by the private DF 53.16 process; checking exit mattered. On a private
50 Hz display, immediate xdotool down/up clicks were missed; a 200 ms press worked.
The copied STANDARD renderer was blank under Xvfb; current init defaults with
AUTO rendered correctly. These were private-test findings, not a reason to
overwrite working live preferences.

Hospital supplies are the abstract hospital location's `contents.count_*` and
`desired_*`, not fortress-wide stock totals. Select existing locations with an
active linked civzone. Soap/plaster/thread/cloth use internal quantity units;
show EMPTY/LOW rather than label them as item counts. Global care requests do
not establish a patient's assignment to the hospital or a treatment-block cause.
DF 53.16 inorganic raws are `world.raws.inorganics.all`, not a direct vector.

Strange-mood requirements come from the native job item flags/materials, with
collected refs indexed by zero-based `job_item_idx`. BAR units are 150 and CLOTH
units 10000. A Stocks count cannot prove material availability or access. Test
temporary native job objects without linking them into the world; clear pointer
vectors before explicitly deleting allocated items and refs.

Stopped script imports must not replace a live collector's state-change handler;
register it only when starting. For rotating marquee messages, a detail phase
counter lets the renderer hold the final visible section and restart even when
the next phase repeats the same text. Count Unicode characters, not UTF-8 bytes,
when measuring the degree glyph's width. Use absolute `dfhack-run` paths from the
source checkout; its working directory is not the game's directory.

Calendar seasons are `df.global.cur_season`: 0 spring, 1 summer, 2 autumn, 3 winter.
Read and validate the native value; never change the user's calendar for a colour
test. Seasonal backlights use the requested RGB values even with persistent health
issues; new-alert flashes briefly use the alarm colour, then return to the season.
The colour override follows the same drawn-frame flash state and six-second
cooldown, so separate clocks cannot make the LCD and backlight disagree. Older
fortress feeds without a season retain their previous urgency colours.

Adventure mode uses the controlled unit from `dfhack.world.getAdventurer()` and a
separate bounded `adv 1` feed. Core/extra party lists and pet lists hold historical
figure IDs; resolve units and deduplicate, excluding the controlled character.
PetOwner alone misses companions' pets. Missing/inactive party members are unknown,
not healthy. Body-part names are `name_singular[0].value`; wounds contain native
zero-based `body_part_id` and `unit_wound_layerst` parts. Creature physiology uses
`caste_raw_flags.HAS_BLOOD`; there is no NOBLOOD flag in this schema.

Native blood_count is signed: combat can overshoot zero (observed -26 on a killed
party pet). Validate its bounded signed value, clamp the displayed percentage to
0..100, and preserve DEAD status; rejecting negative blood blanked the entire feed.
Adventure waking can leave counters.unconscious=-1 (observed on primary1309 after
sleep). Accept that specific signed sentinel and use >0 for KO; retain ordinary
sleep detection separately. Include post-sleep local-map return in regressions.

Urgency is categorical, using named colour bands; do not manufacture HP from wounds
or reuse HP percentage thresholds for blood. Exertion and sleepiness are distinct.
Normal sleep must not trigger the injury-KO rule. Baseline/control switches do not
flash; new serious party incidents can flash without changing the player's sustained
colour. Diagnostic reads must restore alert/session/detail/recovery state, including
mutable recovery tables. Keep fortress seasons independent from Adventure colours.

Travel is Adventure with a loaded world but no local map; getAdventurer() and the
local unit vectors are empty. Resolve adventure.player_id to a nemesis record and
match that nemesis in adventure.player_army_id. army_nemesisst retains live hunger,
thirst and sleepiness plus eats/drinks/sleeps/is_sleeping/on_watch flags; it has no
current blood, wounds, limbs or exertion. The separate travel 1 view must identify
health as unavailable, never replay cached READY/health. Count party HF identities
present in that army, not every army member or historical dead party entry. Reset
view/subject baselines across local/travel changes; include adventure_view in the
diagnostic save/restore. No map alone is insufficient: validate the player army and
matching current nemesis, and wait during loading/unloading without that identity.
