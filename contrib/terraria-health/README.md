# Terraria handler (tModLoader)

`G13TerrariaHealth` is a client-only tModLoader mod that feeds the G13 health meter.
It reports the local player's health and mana with their current effective maxima,
defense, and remaining breath. Equipment, buffs and other mods can change the maxima.
The meter shows an MP readout and gauge, DEF, and an AIR indicator below full breath.
Health remains the primary bar and backlight signal. The mod adds no gameplay
content, world data, or network messages; servers do not need it.

Use g13pad 0.2.46 or newer to read the mana, defense and breath fields. The mod
requires **tModLoader**. A separate observer for vanilla Terraria is described below.

## Build and install

On Linux, use the compiler and .NET runtime included with tModLoader:

```sh
sh contrib/terraria-health/build.sh --version
sh contrib/terraria-health/build.sh /path/to/tModLoader /tmp/g13-terraria-build
```

Copy `/tmp/g13-terraria-build/Mods/G13TerrariaHealth.tmod` into your tModLoader
save directory's `Mods` folder. Enable **G13 Terraria Handler** in Workshop →
Manage Mods and reload mods. The output directory is separate from live saves
because the builder updates its own `enabled.json`. No .NET SDK or NuGet download
is needed. The same `.tmod` is intended for native Linux and Windows under Proton;
install it in each client's actual save directory, which tModLoader logs at startup.

Set a G13 profile's health mode to `feed` and assign that profile to the game's
window class through the editor's Windows menu. Bindings and the saved LCD picture
are kept. No special Steam launch options are required by the mod.

## Feed and lifecycle

An example line is:

```text
240/400 mana 80/200 defense 45 breath 80/200 ttl 3
```

The mod reads the game on its main thread after the update completes. A separate
writer publishes changes at most ten times per second and refreshes unchanged
values once per second, including while paused or unfocused. Menus write `wait`;
death writes zero, with a brief latch for rapid revivals. On unload or exit the last
line expires within three seconds. Writes use a temporary file and atomic rename;
disk failures are logged at most once a minute and retried without blocking game
updates.

The file is `$XDG_STATE_HOME/g13map/health`, or
`~/.local/state/g13map/health`. Under Proton, Unix paths are mapped through `Z:`;
Wine's `WINEHOMEDIR` supplies the host home when `HOME` is absent. The mod refuses
to silently write into a Windows profile when it cannot identify the host home.
`G13MAP_HEALTH_FILE` overrides the path and must be absolute. The private
`/run/user` inside Steam's container is not used.

## Checks

Built and exercised with tModLoader 2026.08.3.0 / Terraria 1.4.4.9 on native Linux
and .NET 8.0.0. An ordinary copied character reports equipment-adjusted mana.
The optional `tools/terraria-fixture/G13TerrariaFixture` mod loads the first player
and world in an isolated save directory, changes health/mana/maxima/breath/defense,
then exercises death, revival, pause, and menu transitions. **Never install that
fixture into your ordinary game.** `tools/check-terraria-health.sh` starts a copied
installation on a private display and records the feed; the Rust meter tests cover
resource parsing, expiry and rendering. Native tModLoader was also checked on the
physical G13. A Steam GE launch of tModLoader stopped at a black screen before
loading the mod; use native tModLoader on Linux.

## Vanilla Terraria on Linux and Proton

The optional `vanilla` observer targets vanilla Terraria's Mono/Windows CLR. It
uses Mono.Cecil to add an observer call at normal returns from `Main.Update` in a
**separate executable copy**. It leaves the original `Terraria.exe` intact and
reads the same player fields through cached reflection, without changing gameplay
fields. It has its own background writer for paused heartbeats. No game assembly
or asset is distributed with this project.

Building requires Mono (`mcs` and `mono`) and Mono.Cecil 0.11. For Windows metadata,
Wine Mono's XNA assemblies must be available; the build script searches the system
Wine and Steam compatibility-tool directories. `G13MAP_CECIL_DLL` can select Cecil.

```sh
sh contrib/terraria-health/vanilla/build.sh /path/to/Terraria /tmp/g13-vanilla-build
cp /tmp/g13-vanilla-build/Terraria.G13.exe /path/to/Terraria/
cp /tmp/g13-vanilla-build/G13TerrariaObserver.dll /path/to/Terraria/
cp /tmp/g13-vanilla-build/original.sha256 /path/to/Terraria/G13Terraria.original.sha256
```

For native Linux, also copy the generated `Terraria.G13.bin.x86_64` into the game
directory. This is a copy of the game's own bundled MonoKickstart runtime; its
name selects `Terraria.G13.exe`. If the builder emits `Terraria.G13.exe.config`,
copy that alongside the executable too. Game-provided runtime binaries stay local.

Use a fresh build output directory. Put `vanilla/launch.sh` in your PATH as
`g13map-terraria`, and set Terraria's Steam Launch Options to
`g13map-terraria %command%`. The wrapper checks the original executable's hash
and substitutes the appropriate native or Windows observer copy in Steam's command.
After a Terraria update or a switch between Steam's Linux and Windows depots,
rebuild and install the copy; the wrapper refuses a stale copy. Removing the
launch option restores ordinary Terraria. The added handler files can then be removed.

The vanilla observer was compiled for .NET Framework 4 and patched Terraria
1.4.5.8. `tools/terraria-fixture/VanillaFixture.cs` exercises real observer calls
through a patched CLR fixture, including both early and final returns, health,
mana, defense, breath, paused heartbeats, death, and menu transitions. It passed
under host Mono and GE-Proton's Windows CLR. Vanilla Terraria 1.4.5.8 was also
verified through its normal Steam native Linux and GE launches on the physical G13.
Native Linux uses the game's bundled MonoKickstart runtime. The wrapper
passes `G13MAP_HEALTH_FILE` explicitly because CLR environment filtering can hide
the host-home variables. Startup and IO diagnostics are written asynchronously to
`G13Terraria.log` beside the observer copy; the Windows GUI game's console output
is not a reliable log.
