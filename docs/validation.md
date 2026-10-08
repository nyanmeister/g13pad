# Validation record

## Development desktop 0.2.9 comparison and upgrade — 2026-10-02

At the owner's request compared installed 0.2.3 against the verified 0.2.9 package,
then updated the main machine. Private Xvfb/i3, copied actual profiles/LCD files and
simulated FIFOs kept all comparison input off the live desktop. Both editors rendered
the saved default profile/LCD/Norman labels correctly; sorted Norman layout JSON was
identical. The new outline has the full palm shape and sloping thumb controls.

Actual old CLI/editor executable: 11,728,352 bytes. New CLI 739,504 plus editor 9,197,104;
together 15.28% smaller. Whole package installed size: 13.51 to 11.77 MiB (pacman reports
a 1.74 MiB saving). Thirty disconnected polls with the same config copy had peak RSS
min/median/max 5,156/5,288/5,448 versus 3,248/3,332/3,472 KiB. Single editor RSS samples
111,772 versus 110,836 KiB do not establish a substantial GUI memory improvement.
Both editors used software rendering at their respective default sizes, not the same
window dimensions. Timing remained below 0.01-second precision.

Root authentication used the machine's sudo-gui helper before service interruption.
Fresh root/user backups and the exact previous 0.2.3 package were retained. All five
installed versions report 0.2.9; installed binaries match the tested package, and running
driver/watcher hashes match installed files. Saved profiles/LCD/preferences, measured
calibration, startup bindings, runtime mapping, panel configuration and unit enablement
were preserved. Actual installed CLI contracts/headless rendering, copied-profile editor
comparison, board sizes/hits and native LXQt icon/text actions passed on private displays.
The live panel displayed the new grey device icon/G13 cross; captured read-only.

No G13 USB/source/FIFOs are currently present on the desktop: the pad remains routed to the
server VM. Driver/watcher are active, waiting; analog is inactive until the source
returns. The old adapter had remained active after removal and was intentionally stopped
as part of the upgrade. The watcher logs repeated expected missing-FIFO LCD retries,
without a restart/crash in these checks. Runtime ownership/modes are g13:g13, 0750/0660.
No hardware routing change, live key/pointer test, physical main-machine press/gameplay,
reboot/login or long soak was performed. Guest 0.2.9 hardware validation above remains
separate evidence. Root auth dialogs were the only deliberate live desktop interaction.

Results, old/new screenshots, detailed failure paths, immediate rollback and the current
checkpoint are in the private maintenance notes (desktop upgrade, 2026-10-02)
and `RESUME.txt`. The earlier 0.2.3 regression reference is now archived there.

## 0.2.9 — lean CLI, configuration and panel parity, 2026-10-02

The CLI/watcher builds without the optional editor dependency graph. `g13map edit`
execs the adjacent editor; text generation and occasional LCD error rendering delegate
to its headless modes. Help/version and saved-profile commands work without hardware
or a display. Configuration commands save files; explicit apply/use changes the device.
Applying a saved profile now clears absent fixed controls even after an offline unbind,
without reconstructing a synthetic previous profile. Invalid profiles are refused
before narrow edits. Raw key sequences and KEY_RESERVED unbind are covered.

Like-for-like source release builds reduced the combined CLI from 11,730,680 to 747,464
bytes, with a separate 9,303,016-byte editor. Together these are 14.33% smaller; the
entire GUI is not 747 KB. The final Arch package uses different packaging/strip flags:
CLI 739,504, editor 9,197,104, helper 402,008, driver 231,392, converter 14,416 bytes.
Thirty disconnected LXQt status subprocesses had median peak RSS 5,200 → 3,332 KiB.
Wall times were below the timer's 0.01-second resolution, so no latency claim is made.

A fresh editor-free production build (driver/converter/CLI/helper, tests off, two jobs,
offline with an already populated registry) took 21.82 seconds, peak RSS 369,612 KiB,
and a 22,360,137-byte Rust target cache. Full GUI builds still need GUI libraries;
one release-profile rebuild took about 4m10 and was not a controlled cold comparison.
Existing mixed caches were retained. The earlier multi-GB observation described build
artifacts, not tracked source. These results support component separation; they do not
compare an equivalent full C implementation.

Source and the Git-free package passed all 14 CTest checks. Editor-free CMake passed
12/12. All 73 Rust checks, including four seeded mutation checks, passed before the
small raw-sequence/unbind follow-up; the final package exercises that follow-up's
ordinary regressions. Formatting, strict full/headless Clippy and shell lint passed.
The command/state/PBM libFuzzer targets each completed 100,000 inputs with ASan, UBSan
and LeakSanitizer enabled, outside the sandbox's unsupported ptrace environment.
These fuzzers are separate development executables, excluded from installation.
Finite inputs cannot certify absence of leaks or exercise real USB timing.

An original device icon and a retraced outline were reviewed iteratively. Private
Xvfb checks exercised normal/minimum board sizes, G2 and sloping thumb-button hits,
Norman physical bindings, German punctuation/dead keys, international capture and
live layout/group changes. Representative Cyrillic/Hebrew/dead-key screenshots were
reviewed for missing glyphs. Native LXQt and installed XFCE genmon checks passed icon
and text clicks plus connected/disconnected rendering on isolated sessions. The live
VM's existing applet displayed the new connected icon/profile without sending desktop
input. Waybar JSON/escaping and plain-text output were contract-tested; native Waybar,
GNOME and KDE applets were not tested or supplied as new tray plugins.

Package functional source is `55cd8d3`; SHA256 is
`dfda0fe9fe91e35645c578aa77df13532f509bc3b02f8de76eaae08daeceefe7`.
Installed only in Arch-Virtual: all five versions report 0.2.9, installed hashes match
the package, and running driver/watcher/xboxdrv hashes match installed files. Real
CLI apply, M-key left/right/keyboard transitions and driver restart passed. All eight
saved user files, both /etc files and the runtime mapping hash were preserved. Services
were active with NRestarts=0; source pointer floated and G13 keyboard stayed attached.
No new physical press, gameplay, login/reboot or long soak is claimed. the desktop's actual
installed CLI remains 0.2.3. The old jstest viewer exited during restarts; it was left
closed to avoid focusing a new window over the user's active VM applications.

The isolated XFCE fixture needed the guest's `xorg-server-xvfb` test package. No live
input, desktop configuration, host USB policy or the desktop installation was changed.
Prior guest 0.2.8 package/configuration backups and measured evidence, failure paths,
screenshots and selective rollback are documented in
the private maintenance notes (`lean-20261002`).

## 0.2.7 — build separation and VM deployment, 2026-10-02

The Rust workspace separates the application, standard-library mapping core and
standard-library analog helper. Normal dev/test profiles retain project line tables
and omit dependency debug information; `full-debug` is opt-in. CMake has independent
editor/adapter targets and options, selectable profiles and configurable Rust jobs.
The Arch recipe tests in the release profile to reuse compiled dependencies.

Measured offline, with two jobs and fresh caches: adapter development build 80.20s to
0.59s; editor test executable 232,499,048 to 45,279,096 bytes; comparable helper-build/
all-tests cache snapshot 1,391,738,880 to 807,989,248 bytes. Full GUI builds still need
GUI dependencies. Cargo's first workspace resolution also needs registry metadata for
the editor on a new machine; selecting the helper avoids compiling those crates.
Existing caches were retained, and later workflows add artifacts beyond the snapshot.
Full-workflow timing comparisons had concurrent work and are illustrative. See
the private maintenance notes (`simplification-20261001`).

The Git-free 0.2.7-1 package passed all 12 checks, strict workspace Clippy, formatting,
core/helper full-debug tests and isolated GUI regression checks. Explicit ignored
mutation tests also passed (56 application, eight adapter, two core tests). Package
SHA256 is `37a98b1f7d7c12e16df869199bba4478e95fe2987b395501f7c5432f3dd6b51d`, built
from functional commit `b6dbe05`; `a9ab9a6` adds only first-build index documentation.

That package was installed in Arch-Virtual, where the real pad is passed through from
the VM host. All four installed executables report 0.2.7, and running driver/watcher hashes
match installed files. The analog helper execs xboxdrv; that running binary matches its
installed file too. Watcher startup with the driver absent recovered automatically;
five subsequent driver restarts restored analog service and the saved profile, with
source pointer floating and keyboard attached. Temporary profile tests exercised
left/L3, right/R3 with swapped/inverted axes, and keyboard mode; runtime xboxdrv
arguments matched the selected mappings. The original saved analog profile was restored.

The installed editor rendered the saved profile, key labels and LCD preview; the
existing XFCE applet reported the connected profile and retained editor actions. These
checks preserved the guest pointer and restored focus. All ten saved configuration/
profile/LCD hashes and the runtime analog mapping remained unchanged.

The owner performed a fresh physical sweep and click. Non-initialization jstest events
showed both source and adapter axes reaching -32767/+32767; adapter X/Y returned to0.
Source button5/BTN_EXTRA press/release matched adapter button9/L3 (release differed by
1ms). Small source movement during clicking remained within the existing deadzone.
The source had 69 X and 67 Y changes; adapter had 38 X and 29 Y changes, with many
intermediate values. A 150-second health capture kept the driver, adapter and watcher
PIDs stable, all NRestarts0, and sampled RSS approximately7.2/9.2/6.0MiB respectively.
Transient missing-FIFO LCD diagnostics during startup/absence recovered; no unexpected
crash, panic or service failure was observed in these checks.

This is bounded VM/hardware validation, not a long-duration soak, new login, SDL/game
test, full-keyboard test or cross-distribution result. the desktop's installed0.2.3 remains
the owner's regression reference. The guest's dedicated 0.2.7 jstest viewer remains
open; rediscover joystick numbers by name after reconnects. Retained0.2.6-1 package,
configuration backup, logs, screenshots and rollback are documented in
the private maintenance notes (`deploy-20261002`).

## 0.2.6 — XFCE panel output

`g13map panel xfce` emits Generic Monitor tags for the same status/profile information
as the existing LXQt output, with themed connected/disconnected G icons and editor actions
on both icon and text. Profile names are escaped as Pango markup; the executable path is
quoted for GLib's command-line parser. Genmon extracts action tags literally, so commands
are not XML entity-encoded. An executable path containing angle brackets or line breaks
cannot be represented by this protocol and is rejected. No-argument LXQt output retains
its original format. Status polls perform local profile/FIFO reads without service calls,
GUI startup, or device writes. No new dependency or persistent process was added.

All ten release CTest checks, strict Rust formatting and Clippy pass. Rendering regressions
cover connected/disconnected/default/non-ASCII profiles, profile text containing markup
tags, and executable paths with spaces, ampersands and quotes. The actual built executable
reports 0.2.6 and produces both panel formats; a single XFCE status invocation completed
below the timer's 0.01-second precision with about 5.2 MB maximum resident memory.
The target VM has XFCE 4.20.8 and genmon 4.3.0 already installed. Its property names and
millisecond interval were checked against the official matching plugin source rather than
obsolete rc-file examples. Interactive panel verification is recorded separately below.

The Git-free 0.2.6-1 package, built from `a1b4e5a`, passed all ten package checks and was
installed only in Arch-Virtual. All four installed executables report 0.2.6; running driver
and watcher hashes match installed files. Generic Monitor plugin 7 was inserted before
the systray on the existing 24-pixel panel, preserving other plugins. It runs
`/usr/bin/g13map panel xfce` every 30000 milliseconds with label hidden and single-row
layout enabled. Writing the plugin array externally required a guest panel restart to
load the native wrapper; genmon 4.2+ settings use xfconf rather than rc files.

Guest screenshots confirmed cyan `13·analog`, then grey with a cross while the driver was
stopped, then cyan after recovery. Both icon and text clicks opened the actual editor.
The two test editors were closed and guest pointer/focus restored. All ten saved G13
configuration/profile/LCD hashes remained unchanged; driver, analog adapter and watcher
are active with adapter NRestarts=0 and source pointer floating. the desktop's desktop and
installed 0.2.3 were untouched. That applet phase tested no physical press or graphical login.
Evidence, screenshots, package checksums and selective panel/package rollback are in
the private maintenance notes (`dev-20261001`).

A subsequent user-assisted physical test on 2026-10-01 used the guest's existing joyutils
1.8.1-4 `jstest` viewer and simultaneous read-only source/adapter captures. With the left
stick/L3 map, both axes reached -32767 and +32767 at the joystick interface and produced
many intermediate values. Adapter axes 0/1 returned to zero. Source button 5 (BTN_EXTRA)
press/release arrived as adapter button 9 (BtnThumbL/L3) at matching event timestamps.
The user noted that clicking the stick tends to move it slightly; the source captured
small movements around the click while adapter axes stayed at zero with deadzone 6000.
No calibration or bindings were changed. This confirms physical analog motion and L3
through the driver/adapter/joydev path, not every pad key, SDL/game mapping, prolonged
jitter behavior or another fresh login. Logs, measured ranges and repeatable procedure:
the private maintenance notes (`dev-20261001`).

## 0.2.5 — stick zone commands during input dispatch

A bound `!stickzone add` command could invalidate the zone vector while a joystick report
was being dispatched. The new regression reproduced a heap-use-after-free under ASan on
0.2.4. Version 0.2.5 retains zone objects and the current iteration list while actions run,
copying the list only when it is changed during dispatch. Deleted zones release their held
action and are skipped; newly added zones join the next report. A stick-mode change stops
the remaining zone dispatch, preventing keys from being pressed after the mode's release.

The regression covers list growth, deleting the current/earlier/later zone, dispatch to
surviving zones, held-key deletion, new-zone activation and a command switching to absolute
mode. All ten release CTest checks and all seven ASan/UBSan checks pass; strict Rust
formatting and Clippy pass. The expanded state fuzzer, including add/delete/mode-change
actions, completed 102,090 inputs (seed 1305, maximum input length 1024) without a sanitizer
error or stranded-key invariant failure. USB transfers and input writes are mocked in
these checks. Local LeakSanitizer cannot run under the terminal sandbox's tracing
restriction, so the local instrumented run disabled leak checking. These results do not
establish a physical stick press or installed-driver behavior for this version.

The committed driver also passed all seven ASan/UBSan/LSan checks in Arch-Virtual with
leak checking enabled. The Git-free Arch package passed all ten CTest checks. An upgrade
in the VM exposed a separate existing watcher-unit race: its FIFO path condition skipped
a restart issued before g13d recreated the FIFO. Package revision 2 removes that gate;
the watcher already follows reconnects, and Restart=on-failure retries initial sends when
M-key modes are enabled. The physical test pad remains passed through to the server VM;
The development desktop's installed 0.2.3 is unchanged.

The driver-phase VM checks used the clean 0.2.5-2 package built from `27279e6`. With M-key modes and
window rules off and the analog profile active, the driver was stopped and its output FIFO
confirmed absent. The watcher started first and remained active; starting the driver then
automatically restored the analog profile and floated the source pointer. Five subsequent
driver restarts kept driver, adapter and watcher active, the G13 keyboard attached, and
the source pointer floating, with adapter NRestarts=0 each time. No manual profile apply
or pointer detach was used during those checks. All ten saved calibration/startup/profile/LCD
file hashes were preserved. Actual installed versions report 0.2.5, and running driver and
watcher hashes match their installed files. Prior 0.2.4 package and configuration backups
are retained. This tests service startup/recovery with real USB hardware; no new physical
stick or key press, new graphical-login boundary, or M-key-enabled cold start is claimed.

## 0.2.3 — login-time session discovery, libevdev key table — 2026-10-01

The first reboot on the packaged units showed both user units starting before the desktop
had exported DISPLAY, XAUTHORITY or I3SOCK to the user manager (LXQt/i3 never activate
graphical-session.target): the watcher could not find i3 (a failure logged every 5 s, window
rules dead, confirmed by the user) and the login pointer detach returned success with nothing
done (the source pointer was attached again). 0.2.3 lets the watcher find its session late and
floats the pointer at login; the driver names keys from libevdev for every enabled code, the
old subset having rejected KEY_VOLUMEUP/DOWN from a saved profile. Release and Git-free
package: 10/10 CTest, rustfmt, strict Clippy. An isolated run with no DISPLAY, no I3SOCK, no
user bus and stand-in FIFOs connected to the live i3 by its runtime-directory socket and
applied the focused window's rule. Installed on the development desktop after the fresh login; the
temporary bridge was removed by the guarded finish-login (runtime files back to 0750/0660, no
ACLs). On the driver restart the adapter unit failed once (`prepare` ran before the new FIFO
existed) and recovered on its restart two seconds later; the helper should wait for the FIFO.
The login path itself (watcher started by systemd at a real login) is verified only by the
isolated run until the next reboot.


## Reconnect pointer correction — 2026-10-01

The user confirmed that unplugging/reconnecting restored the LCD animation and analog
stick. The root service restarted and the watcher restored the default profile and
adapter. Inspection also found the newly created G13 source pointer attached to X11's
desktop pointer. The watcher had restored the profile without repeating pointer isolation.

Version 0.2.2 repeats the existing targeted `pointer:G13` detachment when a new driver FIFO
appears. It retries briefly if X11 has not discovered the pointer yet, and retains the
helper's refusal to detach ambiguous devices. It does not alter the G13 keyboard attachment.
The local release and Git-free Arch package passed all ten CTest checks; strict Rust
formatting and Clippy passed. All four installed executables report 0.2.2. With the corrected
watcher running, a deliberate SIGKILL of the root driver's main process recovered the driver
and analog adapter in seven seconds. The runtime controller mapping remained unchanged,
the running driver matched the installed executable, and the watcher automatically floated
the recreated G13 pointer while its keyboard stayed attached. No manual apply or detach was
performed during recovery. A subsequent physical reconnect again logged automatic pointer
detachment and profile restoration, with both services active afterward. The user has not
reported a separate stationary-mouse observation after the fix; X11 attachment state was
independently checked. The user deferred fresh login to the next session, so that boundary
remains unverified and the temporary session bridge stays in place.

## Development desktop migration — 2026-10-01

Version 0.2.1 changes the original-code license to GPL-3.0-or-later and updates the CS2
documentation. The installed driver, editor, converter and analog helper all report
0.2.1. The running driver and watcher were checked against their installed executables.
Both the local release and Git-free Arch package passed all ten CTest checks; package
versions, desktop metadata, sudoers and isolated service validation passed.

User-assisted hardware checks confirmed right-stick/R3, restoration of default
left-stick/L3, normal LCD appearance, and a temporary error followed by the animation.
A synchronized FIFO trace matched the rendered error payload, then showed a 4.999-second
pause before normal frame writes resumed. The user's disabled error preference was restored.
An earlier trace ended before the stimulus and cannot establish overlay delivery.

The migration preserved profiles, startup bindings and measured calibration, and replaced
the old local service overrides with packaged units. The old package's uninstall script
would delete the service account, so replacement and rollback skipped its scriptlets.
Two preflight mistakes (missing libinput CLI, unnecessary module loading after a kernel
update) exercised rollback; the previous package/services/profile were restored each time.
The corrected helper checks tools and an already-loaded uinput device before interruption.

The existing login lacks the new group in its running processes. Its temporary bridge
grants only the original two adapter service commands and access to the owned runtime
files. the site-specific migration script's `finish-login` step checks both the user manager and i3 process
groups before removing it. The live check helper and migration helper are specific to
the development desktop; they are not a general installer. Reconnect and service recovery were
subsequently checked as recorded above; actual fresh-login and cross-distribution behavior
remain unverified.

## Version 0.2.0 feature and keyboard-layout review

The current source adds per-profile controller output/click/swap/inversion, temporary
LCD error priority/restoration, and active X11 layout/group labels. The 0.1.0 archive
and package hashes below are historical and do not identify this feature release.

- Release build and all ten CTest checks passed; the Rust suite includes mapping
  save/revert/default/profile-switch behavior, invalid mapping destinations, bounded
  Unicode errors and independent-process overlay suppression/deduplication/restoration.
- Strict all-target Clippy, formatting and ShellCheck passed. The seven isolated C++
  checks passed ASan/UBSan/LeakSanitizer outside the ptrace sandbox. The driver action
  proof additionally checks all eight keyboard stick directions and release on centering.
- Explicit Rust mutation runs covered 100,000 profile/parser/FIFO cases with new mapping
  metadata, 100,000 animation cases, 5,000 image and 1,000 text/error-rendering cases.
  Adapter checks covered 100,000 mutations, 10,000 valid configurations and 20,000
  invalid extensions. All passed; these are finite runs rather than exhaustive fuzzing.
- A private Xvfb checked US, Norman, Dvorak, Colemak, French, German, Russian, Greek and
  Hebrew; punctuation, dead keys, Unicode, two effective groups, 90 repeated replacements
  and display absence passed. An independent fixture uses C XKB headers to select and
  verify groups, providing an oracle for the Rust FFI's state structure.
- The same editor stayed open across German/Russian/Greek/Hebrew replacements and group
  changes. Norman's displayed E selection and physical D capture both saved `KEY_D`.
  Screenshot review verified label refresh and Hebrew font fallback; saved bindings
  remained unchanged by layout replacement. No input reached the real desktop.
  Additional physical captures under German (ü and dead acute), Russian, Greek and
  Hebrew saved the expected Linux codes, including `KEY_LEFTBRACE` and `KEY_EQUAL`.
- Staging checked all four 0.2.0 executable versions, sudoers mode/syntax, desktop metadata
  and updated units in a private root fixture. A fresh build under `/opt/G13 Pad` also
  passed quoted service paths and all four staged versions. A Git-free 0.2.0 archive
  built as a local Arch package using its own fresh Cargo cache and passed all ten checks.
  Regenerated dependency notices were identical; Cargo.lock changed only the local version.

The tests exposed and corrected raw-code/Unicode picker filtering and missing Hebrew
glyphs. Native Wayland layout tracking, IME composition and physical hardware transitions
for the new helper remain unconfirmed by this feature validation.

Subsequent user-assisted Steam tests verified **G13 Analog** in the template picker,
its device-specific selection and reload on CS2 focus, and continuous source/Steam axes.
CS2 movement then worked after explicit movement-axis bindings; a local server with
movement quantization disabled preserved partial-to-full speed. The user chose the
digital profile afterward. Deep Rock movement and click were confirmed using its
existing Gamepad layout. See [Steam Input](steam-input.md) for the tested boundaries
and launch-recipe limits. These results used the older installed analog integration;
they do not establish physical behavior of the new helper or a package migration.

## Historical 0.1.0 consolidation checks

The implementation archive is commit `465fb02d17f4d6b89d6ddd36c1472934e2857f63`,
version 0.1.0. Checks ran on Arch Linux x86_64 (the development desktop), using cached,
locked Rust dependencies. This record concerns the consolidated package;
the existing installed editor and patched driver were retained.

## Completed checks

- Release build and all ten CTest checks passed. Rust reported 38 editor tests
  and three adapter tests passing, with three existing fuzz entrypoints ignored.
  C++ proofs cover actions, thread shutdown, manager cleanup and FIFO ownership.
- Clang AddressSanitizer, UndefinedBehaviorSanitizer and LeakSanitizer passed
  all seven driver/utility checks, including the actual driver and converter.
- Strict all-target Clippy, formatting and ShellCheck passed.
- A private Xvfb/i3 session checked text controls, persistence, Escape,
  image background and CLI behavior, without using the real desktop's input.
- DESTDIR staging checked all four executable versions, sudoers permissions
  and syntax, desktop metadata, dynamic libraries, service files, sysusers,
  udev rules and libinput quirks. Service checks used a private root fixture;
  no units were activated or accounts created.
- A Git-free source archive built all components and staged them with an
  installation prefix containing a space (`/opt/G13 Pad`). Its seven C++ checks,
  four staged version outputs and desktop metadata passed. Quoted service paths
  were verified using a private root fixture.
- The local Arch package build passed all ten checks after moving generated
  caches to a filesystem with room. The unpacked package's four version outputs,
  configuration backup entries, sudoers file and desktop entry were checked.
  Its source archive has SHA-256
  `9c7d7b362fe39028a6c76b4d1552a019e86b0d19e820192ad206b02db645646c`;
  the package has SHA-256
  `3d8fa783cab6a2414f3634f8ca2508d70408a89c375a9e036e566a5748493c46`.
  makepkg warned about a source-directory reference retained in the Rust binary;
  reproducible binary output was not established.

## Build lessons

Give each source checkout its own Cargo target directory. Reusing a target
directory across independent copies of the same local package produced an
incorrectly fresh editor binary in this investigation. Clearing that package's
artifacts in the new checkout's own cache fixed it. Checking actual `--version`
output caught the discrepancy; successful compilation alone was insufficient.

Rust release and test profiles can occupy several gigabytes. During package
validation the main filesystem filled and the compiler stopped with an
incomplete LLVM error log. Generated caches were moved to the separate `/tmp`
filesystem, preserving their configured paths with symlinks, before retrying.
The incomplete log does not establish the exact compiler failure cause.
For another machine, choose a build filesystem with sufficient capacity and
pass `-DG13PAD_CARGO_TARGET_DIR=/path/to/private/cache` if necessary. Files in
`/tmp` are disposable; remove stale build-cache symlinks after a reboot.

## Historical 0.1.0 limits

These describe the initial consolidation check; the later migration and hardware checks
at the top of this record supersede its installation and reconnect status.

This is host validation, not a clean Arch chroot or cross-distribution result.
Mocks and private GUI tests do not establish physical reconnect, fresh-login,
gameplay or LCD behavior for the new package. The working installation's earlier
hardware confirmations belong to its historical runbooks. Public hosting and
installation of this package were outside this consolidation step.

## Follow-up lint and fuzz review — 2026-09-30

The consolidated working source passed Rust formatting and strict all-target Clippy,
ShellCheck for current build/tools/package scripts, Perl fixture syntax, and strict
C++ warnings for the converter and its harness. Clang static analysis checked the
converter and driver device, manager, hotplug and LCD boundaries.

Static analysis found stale `errno` use when an opened FIFO failed its identity check.
The FIFO was refused, but its reported error could come from an unrelated successful
operation. A new injected identity-mismatch regression failed before the fix; the
corrected code reports `EACCES`, preserves actual syscall errors, closes the descriptor
and retains the original FIFO. Both injected identity and syscall failures now pass.

All ten release CTest checks passed after this correction. All seven C++ checks also
passed AddressSanitizer, UndefinedBehaviorSanitizer and LeakSanitizer.
The report-thread and manager lifecycle proofs passed ThreadSanitizer as well.
Mutation checks explicitly included the normally ignored entrypoints:

- Rust: 100,000 profile/mode/focus/LCD/text-option/FIFO-framing mutations;
  100,000 animation record/timing cases; 5,000 image-rendering and 1,000 text cases.
- Adapter: 100,000 numeric configuration mutations, 10,000 generated valid
  configurations and 20,000 duplicate/unknown-setting extensions that must fail.
- Clang libFuzzer: 100,000 driver command cases, 100,000 key/stick/profile state
  sequences and 100,000 raw PBM cases, with ASan/UBSan/LeakSanitizer enabled.
  State sequences assert that release/mode exit leaves no emitted key held;
  the converter checks output pixels and padding against independent decoding.

No fuzz crash, sanitizer error or leak was reported in these runs. All 41 editor and
four adapter test entrypoints passed with no ignored tests in the explicit fuzz run.
The optional CMake fuzz targets and invocation examples are in CONTRIBUTING.md.
Runtime FIFO checks use private files, USB/input are mocked, and converter I/O stays
in memory; the live installation was not replaced or exercised by these tests.

These finite seeded runs cover the listed boundaries; they are not a proof of freedom
from defects. The earlier package/source archive hashes above describe the initial
consolidation, before this FIFO diagnostic fix and added fuzz targets.
## 0.2.8 — shared profile transitions and VM USB reconnect

CLI startup, editor apply/revert/switch and watcher switching use one application plan.
The plan retains the desired profile, prior live profile and driver baseline separately;
it adds explicit unbinds only while constructing commands. Adapter transitions finish
before TOP ownership is decided, M-key routes override profile actions last, and active
profile commits and LCD playback remain with their callers. Login preserves profile LEDs
until the watcher selects a sum; the editor uses sum zero and the watcher its current sum.
FIFO commands are not a transaction and the executor does not promise rollback of a
partially sent profile. The adapter's existing mapping/service rollback remains intact.

Regression fixtures cover startup routing/stick preference, unsaved-editor revert,
baseline and previous-profile unbinds, TOP ownership, watcher switches and failure before
committing the active profile. Optional host USB policy fixtures cover stale/current/
absent attachments, a stopped VM, wrong device, failed detach and invalid kernel name.
On 2026-10-02 the host rule passed a user-assisted physical replug: live USB address
4:4 became 4:5 and the rule detached/reattached the G13 automatically. Guest 0.2.7
driver, adapter and watcher recovered; the watcher restored the saved analog profile
and floated the source pointer. Saved configuration hashes stayed unchanged.
This establishes reconnect policy on the running VM; absent-pad VM startup was not
exercised by restarting the guest. Installed 0.2.8 integration is recorded separately.

The committed 0.2.8 source/package passed all 12 CMake/package checks, 66 ordinary Rust
tests plus four explicit seeded checks, formatting and strict all-target workspace Clippy.
A private Xvfb fixture verified Norman selection/capture/save, German punctuation/dead
keys, Cyrillic/Greek/Hebrew captures and live layout/group changes; representative rendered
glyphs were visually reviewed. No real desktop input was generated.

Arch-Virtual installed 0.2.8-1 from functional commit `1c84576`. All four installed versions
were checked. The real VM fixture exercised CLI apply and watcher M1 → right/R3 with
swapped/inverted axes, MR → left/L3, M2 → keyboard, MR → left and one driver restart.
The watcher restored its profile, reopened the output FIFO, and kept the source pointer
floating with keyboard attached. Original analog profile, all ten saved/runtime hashes,
and calibration were restored. Running driver/watcher hashes match the package; the
adapter executes the installed xboxdrv. The XFCE applet returned the original profile.
`tools/check-vm-application.sh` documents/repeats the bounded integration fixture.
No new physical all-key/gameplay test, cold VM boot or extended soak is claimed.
