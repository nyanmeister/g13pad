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
`game/csgo/cfg/`; `g13map health cs2-config` prints it. A profile in health mode `cs2`
makes the watcher run the listener itself while the profile is active, so
nothing has to be started by hand. `player.state.health`, `armor` and `helmet` are the
fields; while dead, `player` is whoever is being spectated (compare `player.steamid`
with `provider.steamid`), and the menu has no `player.state` at all. Counter-Strike:
Source has the same armour and helmet but no Game State Integration; it needs another
route (a server plugin), and a profile in mode `cs2` there waits forever.

## Doom-family engines: the console is the channel

ZDoom-family engines (Zandronum, GZDoom) have no integration feed, but ACS can print
to the console and the console can go to a file (`+logfile PATH`). `contrib/doom-health`
is a tiny ACS library loaded with every game through `LOADACS` that prints a tagged
line, dark grey, only on a change (the console is the player's too; the follower keeps
the feed alive between lines); health mode `log` makes the watcher follow the newest
file with that name. Lessons: a PK3 is a zip, and `bsdtar -a` writes a *tar* for an
unknown extension (the engine then lists the file with no lumps and nothing runs);
an ACS library must start with `#library "NAME"`, or its string constants resolve
against another mod's table (with Brutal Doom loaded the tag came out as its weapon
strings with my numbers between); Zandronum appends a timestamp to the log name unless
`sv_logfilenametimestamp` is off; Zandronum 3.2 returns 0 for `APROP_SpawnHealth`; the console is not on stdout
there, so look at the logfile, and test on a private X display with a copied config
(`-config`), because the engine rewrites its ini on exit.

## GoldSource: intercept both client export routes

`contrib/goldsrc-health` is a native preload observer for interface 7. The ordinary
HUD channel sends `Health` (byte, or a short in some mods) and `Battery` (short).
It wraps registration while `Initialize` copies the engine table, restores that
table, and passes every message unchanged to the mod. The Steam client exports its
callbacks through `F`: wrapping only individually looked-up functions passes a
simple fixture but does nothing in the game. Cover both routes. Keep caller-relative
`dlsym` lookups as tail calls; otherwise `RTLD_NEXT` can start from the wrong module.
Old glibc versions also need explicit `libdl.so.2`/`librt.so.1` dependencies, not only
old symbol versions (modern glibc's `-ldl -lrt` may be empty stubs). The launcher uses
literal `$LIB` for architecture selection and adds `-insecure`; keep this hook to
singleplayer/trusted co-op. Use a copied installation/config on a private X display,
`G13MAP_HEALTH_FILE` to isolate its feed, and disable joystick input. Startup videos
and a locale warning can block a map command; inspect the private window before
concluding the hook failed.

GoldSource Steam-launch correction (2026-10-09): a versioned libc `dlsym` bootstrap
can bypass Steam overlay's unversioned interposer, leaving its `close` hook
uninitialized. Host-shell substitutions then retain pipe writers and hang before
the game starts. Bootstrap the next UNVERSIONED `dlsym` through libc and tail-call
it for unrelated symbols. Reversing preload order launched the shell but bypassed
the health observer; clearing preloads confirmed the collision but is diagnostic,
not the final fix. Test actual Steam argv plus inherited overlay, not only a
wrapper inside a pre-established container with `LD_PRELOAD` cleared. The new
constructor/chaining fixtures catch the old observer on both architectures.

## Terraria: distinguish the two runtimes

`contrib/terraria-health` contains a client-only tModLoader mod (.NET 8) and a
separate vanilla Windows CLR observer (.NET Framework 4). Native tModLoader may
use `dotnet` as its window class; Proton uses the Steam application class. The
editor owns the LCD while open, so close it before judging the live meter.

Read player fields after the game's update, including effective health/mana
maxima, and publish from a separate timer so actual paused updates keep a live
heartbeat. The vanilla patcher preserves the original executable and branch
targets at all normal returns, widening short branches. Cecil needs both embedded
game assemblies and Wine Mono's XNA/FNA metadata to rewrite the assembly. Test
with `tools/check-terraria-vanilla.sh`; never install the gameplay fixture mods
into ordinary saves. A stale executable copy must be rejected after game updates.
Pass the host feed path explicitly through `G13MAP_HEALTH_FILE`: .NET Framework's
environment can hide HOME/WINEHOMEDIR even when the host process has them. Windows
GUI console handles can discard errors; log asynchronously beside the copy.

## Unity games: BepInEx (ULTRAKILL)

`contrib/ultrakill-health` is the model: a BepInEx 5 plugin, one `MonoBehaviour` whose
`Update` composes the line ten times a second from the game's singletons and writes it.
What made it work:

- **Read the assembly, not memory.** `monodis Assembly-CSharp.dll` (Mono is packaged)
  lists every class and field; four of my six guessed names were wrong before I looked
  (`currentStyle` is `currentMeter`, the rank is a private `_rankIndex`, hard damage is
  `antiHp`, dashes are `boostCharge` 0–300). Private fields are one `GetField` with
  `BindingFlags.NonPublic` away; no Harmony patch is needed just to read.
- **Mono's `csc` builds the DLL** against the game's own `ULTRAKILL_Data/Managed/*.dll`
  and BepInEx's `core/` (`-nostdlib -noconfig`, reference `mscorlib`, `System`,
  `System.Core`, `netstandard`, `UnityEngine`, `UnityEngine.CoreModule`,
  `Assembly-CSharp`, `BepInEx`); no .NET SDK, no NuGet.
- **Proton:** the game is a Windows build, so BepInEx is the `win_x64` zip and Steam's
  launch option `WINEDLLOVERRIDES="winhttp=n,b" %command%` makes Wine load the doorstop
  proxy. The host's home is `Z:` plus the Unix path, but Proton passes no `HOME` into
  the game (the first launch said so); Wine's own `WINEHOMEDIR` (`\??\unix\home\NAME`)
  is there, and a `[Feed] Path` in the plugin's BepInEx config overrides either. When a
  path guess fails, log the environment's variable names: the next round is one launch.
- **The game destroys the plugin's component.** ULTRAKILL disables and destroys
  BepInEx's manager object soon after startup (the log says so once OnDisable and
  OnDestroy are logged); a plugin `Update` never runs, and it took three launches to see
  that the silence was the component, not the logger or the file. So: do the reading in
  a Harmony postfix on the player's own `Update` (`AccessTools.Method(typeof(NewMovement),
  "Update")`) and keep the file alive from a `System.Threading.Timer`; both outlive the
  component. Static state, a lock around the file. On an unknown game, log OnDisable and
  OnDestroy from the first build.
- **Menus have no player:** the postfix does not fire, so the timer writes `wait` once
  1.5 s pass without a frame. Write on change and once a second regardless, with
  `ttl 3`, so the meter ends with the game even if `OnApplicationQuit` never runs.
- A look per game is the profile's own tuning lines (`meter.d/PROFILE`: `sprite v1`,
  `heart 2 1`); the extra words draw their own elements, so no layout switch exists.

## Choosing where it shows

The meter is a profile's **Health mode** tick with a reader (`g13map profile health NAME
feed|cs2|log`; the picture stays kept underneath), so the window rules put it on the
game's windows and nowhere else. Two profiles with the same reader
hand the meter across a switch; any other picture ends it, and the ending profile's
own backlight comes back (the meter was once restoring the colour of the profile it
started under, over the new one: if a colour "leaks" between profiles, look there).

## The OBS overlay and the driver's state files

`g13map obs` (src/obs.rs, 0.2.39) draws the pad for an OBS window capture. What it
took, for the next thing that wants to show the pad or the glass on a monitor:

- **The input-overlay plugin cannot drive it.** Its gamepad path (SDL2
  `SDL_GameController` in the installed 5.1.0; SDL3 `SDL_Gamepad` upstream) stops at
  the 21 mapped buttons, and its keyboard path (libuiohook) sees the profile's bindings,
  not the keys, so a key-code layout would change with every profile and mode. The
  overlay is therefore our own window, with the plugin's *asset shape* kept (a PNG
  sheet, a JSON layout, pressed sprite 3 px below) so the art can be repainted the
  same way as the plugin's presets.
- **The driver publishes its state as files beside the pipes**, like the health feed:
  `g13-0_keys` (`stick X Y`, `backlight R G B`, `keys NAME...`; the text is rewritten
  atomically on change, from the raw report bits in `G13_KEY_STRINGS` order) and
  `g13-0_lcd` (the 960 bytes last sent to the glass). Not the output FIFO: `g13map
  watch` already reads that, and a FIFO splits its bytes between readers. The names
  are set at the top of `RegisterContext`, because `LcdInit` sends the logo and
  `SetKeyColor` runs before the pipes exist and both are state. A proof
  (`tools/driver-keystate-proof.cpp`) pins the bit order; it is exempt from the
  `--wrap=write` the other proofs link with.
- **The sheet is generated from the editor's board** (`board::spots`, the traced
  outline) on a 4-pixel grid, the grammar read off the plugin's pixel keyboard: a
  1-unit outline, a bevel light at top-left and dark at bottom-right, 3x5 glyphs, cyan
  ink when pressed. `g13map obs --dump DIR` writes it; a test keeps `assets/obs/`
  equal to the generator, so regenerate after changing the art.
- **The glass table** (src/glass.rs, `~/.config/g13map/glass`) is the one place that
  translates LED values to monitor colours; draw the LCD through `Glass::render`
  rather than inventing colours. A lit pixel is the background's light `glow` times
  over (2 by default), the same colour brighter, going pale only where a channel has no
  headroom; the old blend toward white desaturated, which the owner saw at once beside
  the glass. `g13map glass` is the by-eye course that fills the file.
- **The OBS plugin** (`obs/g13pad-obs.c`, 0.2.41) is the real answer to "a source like
  the others": libobs C API, `gs_image_file4_init` with `GS_IMAGE_ALPHA_PREMULTIPLY`,
  blend ONE/INVSRCALPHA with sRGB framebuffer like image-source, one
  `gs_draw_sprite_subregion` per element, a dynamic 160x43 `GS_RGBA` texture for the
  LCD updated in `video_render` (the graphics context is sure there). jansson parses
  the layout: `obs_data` arrays hold objects only, so `pos: [x, y]` is unreadable
  through it. Arch's `obs-studio` package carries the headers and the CMake package
  (`OBS::libobs`).
- **Testing OBS headless touched the live config once.** `HOME=` alone is not enough:
  this session exports `XDG_CONFIG_HOME=$HOME/.config`, so an OBS started with a
  scratch HOME loaded and *saved* the real profile and scene collection (restored from
  `Untitled.json.bak`; the ini files were rewritten and could not be checked). Set
  `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` to the scratch tree, confirm
  the process's environ before trusting it, and kill it with `-9` if it is wrong, so it
  never saves on exit. User plugins load from
  `$XDG_CONFIG_HOME/obs-studio/plugins/NAME/bin/64bit/NAME.so` with `data/locale`
  beside; a scene-collection item copied from a group needs `group_item_backup: false`
  or it stays hidden.
- **Check the window carries alpha** with a raw `xwd` dump (ImageMagick drops the
  alpha byte of a depth-32 window): the corner pixel reads `00000000`, the body
  `2c2c2cff`. `import -window` composites over black and proves nothing.

## Iterating without the package cycle

- `~/.config/g13map/meter` holds the look (the band ladder and its colours, the dark hold, beat
  spacing, the swell, the flash, where the heart and the readout sit, the CS2 port) and is re-read within two seconds while the meter runs.
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

## Factorio: observe a verified native executable

`src/factorio_main.rs` is a read-only external Rust observer and launch wrapper,
not a Factorio mod. Linux ships function symbols and line DWARF, but not the game
class layouts: gdb's initial indexing is costly and does not recover those types.
Derive fields from named accessors and resolve relocated globals and class markers
from ELF symbols. The build ID is informational: try new native builds with the
last member layout, validating pointers, object types, bounded collections and
finite numbers. Keep HP bounds generous for mods and permit overheal/overcharge.
Ten consecutive bad snapshots disable telemetry for that address space; menu/loading waits
reset that counter. A mod-sync restart can exec the same binary with the same PID:
an open /proc/PID/mem handle still refers to the old address space and returns EOF.
Probe the executable mapping even after disabling telemetry and reopen only when
that handle retires; do not confuse stale memory with incompatible game data.
Verify same-PID exec, changed IDs, moved symbols and junk data in a private
scenario. Read `/proc/PID/mem` only;
never loosen ptrace policy or install a privileged observer. The production
wrapper is the game's ancestor, including Steam's runtime wrappers.

The equipment vector contains every equipped item. Classify shield/battery
objects before reading their fields; an empty armor inventory has no grid. Health
is a ratio multiplied by the effective maximum (quality and force/character
bonuses). Use the local player's force, and retain their character controller
while remote view is active. Research can be science or a trigger; craft-item and
craft-fluid triggers have separate counters. Experimental 2.1 makes
`current_research` read-only: the fixture queues it with `add_research`.

CharacterController::getVehicle reads character+0x550. Vehicle health is a ratio
at +0x80, with prototype+0x48/max+0x630 and quality+0x91. Resolve and validate
Car/train/spider class markers before reading, and verify getting out, passenger,
quality and remote transitions in the private vehicle fixture. Vehicle HP becomes
primary; suit shield/battery stay personal, and the small pilot HP remains visible.
Do not apply the character's force/individual HP bonuses to a vehicle maximum.
Vehicle rendering suppresses ECG and death holds from a destroyed vehicle.
The default vehicle icon is a chain of three counter-rotating gears; damage cracks
follow the upper edge of the named orange band, with 50% as the fallback. Keep
cracks on the rotating bodies and clear them when health recovers above the band.
Research labels are bounded to 256 ASCII characters and marquee inside a clipped
window with fixed progress; keep byte/glyph bounds and long-uptime animation tests.

The attack signal is `entity_under_attack`, not every alert category. Its renderer
owns a monotonic six-second cooldown so clearing/reappearing alerts cannot retrigger
it early. Check actual text glyphs too: the old small font contained only style
and Terraria letters, turning BAT/ATTACK/research names into dashes. An exported
frame caught that despite passing compilation and unrelated meter tests.

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
