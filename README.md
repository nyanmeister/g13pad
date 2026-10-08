# g13pad

A Linux driver and configuration app for the Logitech G13: key bindings, profiles,
backlight, LCD images/animation/text, and an optional Xbox-compatible analog stick.
The editor is launched with **`g13map edit`**; existing `~/.config/g13map` profiles are retained.
The small `g13map` CLI/watcher and optional `g13map-editor` executable are separate.

The repository contains the tested C++ driver, Rust editor, analog adapter integration,
Linux service/permission files, and isolated regression checks. Builds and package staging
have no service or desktop side effects. i3 window rules and LXQt/XFCE applets are optional;
the editor works independently.

## Build and check

On Arch install the build dependencies `base-devel cmake ninja rust libusb libevdev log4cpp`
and the GUI runtime dependencies listed in [the Arch recipe](packaging/PKGBUILD), including
`libglvnd libxkbcommon libxkbcommon-x11 libx11`. A first Rust build downloads the
versions pinned by Cargo.lock. For an already populated cache, add
`-DG13PAD_CARGO_OFFLINE=ON`.

```sh
./build.sh -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_INSTALL_LIBDIR=lib
./check.sh
```

Use `G13PAD_BUILD_DIR` and `G13PAD_JOBS` to select a build directory/job count.
Rust inherits that job count; override it with `G13PAD_RUST_JOBS` or CMake's
`-DG13PAD_RUST_JOBS=2`. The conservative default remains two jobs.
`-DBUILD_TESTING=OFF` omits tests. Editor and adapter builds are independent:
`-DG13PAD_BUILD_EDITOR=OFF` keeps the C++ components, CLI/watcher and small analog helper;
also disable `G13PAD_BUILD_CLI` and `G13PAD_BUILD_ADAPTER` for C++ only. In a configured full build,
`cmake --build build --target g13d`, `cli`, `editor`, or `adapter` selects just that component.
`-DG13PAD_SANITIZER=address,undefined` with Clang enables
instrumented driver checks. The proofs use mock USB/input devices and private files;
no keys or pointer input reach the desktop.

The Rust workspace contains the application (`g13map`), standard-library mapping model
(`g13pad-core`) and standard-library adapter (`g13pad-analog`). For a quick helper check:

```sh
cargo test --offline --locked -p g13pad-core -p g13pad-analog
```

See [build profiles and caches](docs/building.md) for lean development tests, full debugger
information and release/package cache reuse. A full editor build still needs GUI libraries.

## Stage and install

```sh
DESTDIR="$PWD/build/stage" cmake --install build
```

Inspect the resulting files and `build/install_manifest.txt`. Staging creates a package
root; it does not activate services. Prefer an OS package over installing onto an existing
system by hand: CMake's installation copies configuration files and does not merge them.
See [installation](docs/installation.md) and [migration](docs/migration.md) first.

For a local Arch package, export a committed source tree as `g13pad-0.2.16.tar.gz`, put it
beside `packaging/PKGBUILD`, and run `makepkg` in that directory. The recipe uses a local
archive with a placeholder checksum; a public release must supply a verified checksum.
An Arch package preserves changed startup bindings/calibration through pacman's backup
mechanism. xboxdrv is an optional separate dependency, not bundled.

## Use

```sh
g13map --help                       # all profile and panel commands
g13map profiles                     # list saved profiles
g13map profile create gaming default
g13map profile bind gaming G1 KEY_D  # physical key code; layout labels: g13map layout
g13map profile stick gaming analog
g13map use gaming                   # apply and select; clears absent bindings
g13map edit                         # editor
g13map import FILE NAME             # keep an existing driver's bindings
g13map apply                        # apply the selected saved profile
g13map watch                        # profile switching and LCD playback
g13map marquee 'Message' NAME        # keep reusable LCD text, no hardware write
g13map --version
g13map panel xfce                   # XFCE Generic Monitor status and editor button
g13map panel waybar                 # Waybar custom-module JSON
g13map panel text                   # i3blocks/Polybar/tint2 or terminal status
```

`profile` commands save files; `use`/`apply` sends settings to the device. Syntax/configuration
errors go to stderr without changing the LCD. Missing hardware does not prevent saved
configuration or help/version commands. `g13map status` shows the selected profile,
connection and paths. The shared profile format remains editable with any text editor.

The editor offers captured keys/chords, raw daemon actions, M-key additive profile modes,
i3 window rules, per-profile stick preference and LCD choices. Images support crop,
background, threshold/dither, inversion and animation. Text supports installed fonts,
size, multiline wrapping/alignment and scrolling. **Animations…** offers built-in looping
pixel art drawn in code (a rainy skyline, a starfield, Pong, Life, waves, cubes, digital
rain, a heartbeat, an aquarium, tesseracts); a click keeps one as a picture. `g13map-anim` lists and
keeps the same scenes from a terminal. `g13map-apply.service` restores the
saved profile at login; `g13map-watch.service` handles automatic switching/animation,
temporary LCD error recovery, and profile reapplication after a driver reconnect.

For XFCE, add a Generic Monitor item, set its command to `g13map panel xfce`, hide its
extra label, and use a 30-second update interval. It shows the device icon, active profile and
driver availability; clicking the icon or text opens the editor. LXQt's existing
custom-command output remains the default with no arguments. See
[panel setup](docs/installation.md#panel-applets) for settings and optional dependencies.

The key picker, board and chord labels follow the active X11 keyboard layout/group, including
changes while the editor is open. Bindings retain physical Linux `KEY_*` codes: Norman's
displayed E is `KEY_D`. The raw code is shown beside the label. `g13map layout` prints the
current translation without requiring a G13. Without an X display, labels use physical
names; native Wayland layout tracking is not implemented. Games may interpret keys
differently, so confirm their own bindings.
Dead keys are identified explicitly. Fontconfig selects an installed fallback font for
international labels; install fonts covering the scripts you use (for example DejaVu Sans
for Hebrew). IME composition and shifted/Caps Lock text are not modeled by these
unshifted key labels.

Analog support defaults to the left controller stick and its click to L3, preserving
normal board keyboard events. **Controller…** changes the output stick, click button,
axis swap and physical-axis inversion per profile. Apply changes the live adapter; Save
keeps them, Revert restores the saved mapping, and Use defaults removes the override.
Calibration continues to follow physical X/Y when axes are swapped.
`/etc/g13/analog.conf` contains a dead zone and optional
**device-specific** calibration. `g13pad-analog check FILE` validates it without accessing
hardware. The updated helper/services and group access are required for custom mappings.
**Show temporary LCD errors** displays errors for five seconds, then restores the latest
normal image/animation frame. The watcher performs restoration even after the editor
closes and with automatic profile switching off. The setting is enabled by default.

An optional [G13 Analog Steam Input template](packaging/steam-g13-analog.vdf) preserves
continuous gamepad axes instead of converting them to keyboard directions. See
[Steam Input setup](docs/steam-input.md). A template cannot make a game consume analog
input if its controller input path is inactive.

## Current limits

Validated on Linux x86_64 with systemd; i3 rules target i3 and pointer isolation targets
X11/libinput. One G13 and one controlling login session are the initial integration scope.
The inherited FIFO protocol requires coordinated writes; unrelated direct writers or
multiple controlling sessions can bypass the editor's lock. Fresh login/reconnect and
cross-distribution installation require hardware/environment-specific checks.

The physical test pad is connected to a server and passed through to its Arch-Virtual VM,
isolating driver faults, generated input and GUI tests from the working desktop.
Optional [host-side USB reconnect rules](docs/vm-usb.md) refresh a stale live attachment
after a physical replug; these files are not installed by the desktop package.
Version 0.2.4 was hardware-verified there, including fresh graphical login and five driver
restarts with analog active. The VM now runs 0.2.9-1, retaining the stick-zone command crash
and watcher FIFO startup fixes, with a native XFCE Generic Monitor applet. Icon and text
clicks opened the editor; driver stop/recovery changed its grey/cyan status correctly.
A user-assisted jstest capture on 0.2.6 confirmed both physical stick axes across their
full joystick-interface range, return to adapter centre, and L3 press/release.
After the 0.2.7 workspace/build changes, VM checks repeated missing-driver startup,
five driver restarts, left/right/keyboard mapping transitions and an installed-editor
check. A new physical jstest capture confirmed both axes and L3, with unchanged saved
configuration and no automatic service restarts during a 150-second health check.
Version 0.2.8 unifies CLI, editor and watcher profile transitions without constructing
synthetic saved profiles for unbinds. Installed VM checks verified CLI apply, M-key-driven
left/right/keyboard transitions, driver restart recovery and restoration of the saved
profile/runtime mapping. Host USB reconnect automation passed a physical replug while
the guest still ran 0.2.7; its driver, adapter and watcher recovered automatically.
Version 0.2.10 adds built-in looping LCD animations drawn in code (0.2.11: tesseracts; 0.2.12: pixel-scrolling digital rain, calmer skyline windows; 0.2.13: digital rain as endless ribbons, no reset; 0.2.14: aquarium fish take breaks, a pufferfish stays; 0.2.15: waves gull rests, fish jump; 0.2.16: scenes never overwrite kept text, keep is all-or-nothing) (**Animations…** in the
editor, `g13map-anim` in a terminal) behind the `art` feature, with a test that every loop
closes. Kept pictures and profiles are unchanged; a scene is kept like any other picture.

Version 0.2.9 separates the CLI/watcher from the editor and adds saved-profile configuration
commands, a device icon, a retraced board, and Waybar/plain-text panel output. The packaged
CLI is 739,504 bytes; the optional editor is 9,197,104 bytes. Installed VM profile/mode and
restart checks passed again. Native LXQt/XFCE icon and text clicks, connected/disconnected
rendering, and editor controls were checked on private displays. Three driver/converter
fuzz targets completed 100,000 inputs each with ASan, UBSan and leak checks enabled.
Waybar JSON and plain text were format-tested; native Waybar and GNOME/KDE integrations
were not exercised. Full editor builds still compile GUI dependencies.
Missing-driver startup and five further restarts passed on 0.2.5-2.
The development desktop was updated from 0.2.3 to 0.2.9 after comparing its actual saved profiles
on a private display. Saved files, calibration, runtime mapping and panel settings were
preserved; installed CLI/editor/native LXQt checks passed. The pad remains routed to the
VM, so the desktop's driver/watcher wait for it and its analog adapter is stopped. No physical
0.2.9 check on the desktop is claimed. Its temporary migration access bridge was removed after
login credentials were verified. Earlier user-assisted
checks confirmed right-stick/R3 and restored left-stick/L3 mapping, LCD error restoration,
and recovery after physical reconnect. Automated mocks do not establish physical gameplay
or cross-distribution results.
See the [validation record](docs/validation.md) for checks, limits and build-cache lessons.

## License and contributions

Original code and modifications are **GPL version 3 or later**, chosen by the project owner.
Upstream public-domain/MIT notices and dependency/font licenses are retained. See
[LICENSE](LICENSE), [NOTICE](NOTICE.md), and [source provenance](docs/provenance.md).
Original device artwork replaces upstream branded/game assets.

[Contributing](CONTRIBUTING.md) explains checks, useful bug reports and how source is
published. The private development history, migration records and site-specific upgrade
scripts stay with the owner's maintenance notes; this README describes the consolidated source.

## Authors

- **nyanmeister**: owner, design, every hardware check and every report from the glass.
- **Claude (Fable 5.1, Anthropic)**: the `g13map` editor, CLI and watcher, LCD pictures,
  text and the built-in animations, i3 window rules, M-key profile modes.
- **Codex (GPT-6, OpenAI)**: driver consolidation and the imported fixes, packaging,
  services and permissions, the lean CLI split, the analog adapter, validation records.

Commits carry the assistant and model that wrote them.
