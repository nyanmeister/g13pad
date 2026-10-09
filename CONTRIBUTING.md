# Contributing

Original contributions use GPL-3.0-or-later. Preserve imported notices and update provenance
when bringing in third-party material. Keep the g13map command and profile compatibility
unless a migration is explicitly designed and tested.

Build with `./build.sh`, run `./check.sh`, and run Rust formatting/strict Clippy:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Driver proofs exercise action release, ordered USB reports, hotplug/thread ownership,
FIFO permissions and failed startup using mocked USB/uinput. Keep assertions enabled in
proof builds. Clang ASan/UBSan builds and the existing fuzz harnesses are useful when changing
those boundaries. LeakSanitizer may require execution outside a ptrace sandbox. GUI checks
must use Xvfb/private i3/FIFOs/configuration, not a user's live keyboard or pointer.

Keyboard layout regressions have repeatable private-display checks:

```sh
mkdir -p build/layout-review
sh tools/check-layouts.sh build/layout-review
sh tools/check-layout-gui.sh build/layout-review
```

They require Xvfb, XKB utilities, a C++ compiler/X11 headers, jq, and (for the GUI)
i3, xdotool, ImageMagick and Tesseract. The first checks nine layouts, punctuation,
Unicode, effective groups, repeated replacements and display absence. The second checks
Norman's picker/capture/save codes and keeps one editor open across layout/group changes;
review its screenshots for label refresh and font coverage. All input stays on its own
display and the service backend uses private simulation/FIFOs.

Board and native panel checks also use private displays/configuration:

```sh
sh tools/check-board-gui.sh build/board-review
G13PAD_TEST_BINARY=/usr/bin/g13map sh tools/check-xfce-gui.sh build/xfce-review
```

The board check requires i3, xdotool, ImageMagick, Tesseract and XKB tools, and exercises
LXQt when `lxqt-panel` is installed. The XFCE check requires Xvfb, dbus-run-session,
xfwm4, xfconf-query, xfce4-panel/genmon, xdotool, xwd and Perl. It defaults to the
installed CLI/editor pair; override `G13PAD_TEST_BINARY` for a built pair. Both check
icon/text launch actions and connected/disconnected rendering. Review screenshots;
successful process launch alone does not establish appearance parity. Convert XFCE
XWD captures locally with ImageMagick when the test machine lacks it.

Window rules without i3 have a check of their own, on a private Xvfb display under an EWMH
window manager (openbox unless `G13PAD_TEST_WM` names another, such as `xfwm4`):

```sh
G13PAD_TEST_BINARY=build/rust/release/g13map sh tools/check-focus-x11.sh build/focus-review
```

It requires Xvfb, the window manager, xterm, xdotool, xprop and Perl, and exercises
`g13map focus windows` and a `g13map watch` switching profiles by focus against the
stand-in daemon pipes; nothing touches the live display.

The same under a headless Wayland compositor (labwc unless `G13PAD_TEST_COMPOSITOR` names
sway, wayfire or river; niri nested with `G13PAD_TEST_PARENT=sway`), with foot as the client:

```sh
G13PAD_TEST_BINARY=build/rust/release/g13map sh tools/check-focus-wayland.sh build/wfocus-review
```

For a before/after check with real configuration copies, use
`tools/check-local-comparison.sh OLD_BIN_DIR NEW_BIN_DIR CONFIG_COPY OUTPUT_DIRECTORY`.
It compares Norman layout JSON, 30 disconnected panel polls, and editor screenshots
using Xvfb/private i3/FIFOs. The copy is never edited in place. Include the separate
editor beside the new CLI. Review both screenshots and distinguish a single GUI RSS
sample from a controlled memory benchmark; the helper does not establish real USB input.

Run the deterministic Rust mutation checks, including adapter configuration, explicitly:

```sh
cargo test --offline --locked --workspace -- --test-threads=1 --include-ignored --nocapture
```

Clang libFuzzer targets compile the actual driver/converter sources with coverage,
AddressSanitizer and UndefinedBehaviorSanitizer. USB transfers/input writes are mocked;
the converter uses memory streams and checks its output against decoded PBM pixels.
These targets are opt-in development checks and are not installed:

```sh
cmake -S . -B build-fuzz -G Ninja -DCMAKE_CXX_COMPILER=clang++ \
  -DCMAKE_BUILD_TYPE=Release -DG13PAD_BUILD_EDITOR=OFF -DG13PAD_BUILD_CLI=OFF -DG13PAD_BUILD_ADAPTER=OFF \
  -DG13PAD_BUILD_FUZZERS=ON
cmake --build build-fuzz -j2
mkdir -p build-fuzz/corpus/command build-fuzz/corpus/state build-fuzz/corpus/pbm
printf 'bind G10 KEY_LEFTCTRL\nbind G10 KEY_RESERVED\n' > build-fuzz/corpus/command/bind
printf '\001\002\003\004\005\006\007' > build-fuzz/corpus/state/events
{ printf 'P4\n160 43\n'; dd if=/dev/zero bs=860 count=1 status=none; } > build-fuzz/corpus/pbm/image
ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1 \
  build-fuzz/command-fuzz build-fuzz/corpus/command -runs=100000 -max_len=4096
ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1 \
  build-fuzz/state-fuzz build-fuzz/corpus/state -runs=100000 -max_len=4096
ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1 \
  build-fuzz/pbm-fuzz build-fuzz/corpus/pbm -runs=100000 -max_len=16384
```

Keep seeds, crash artifacts, run counts and sanitizer output with review evidence.
`sh tools/check-fuzz.sh build-fuzz OUTPUT_DIRECTORY 100000` seeds and runs all three
targets with ASan, UBSan and LeakSanitizer; logs and crash artifacts remain in that
directory. Run outside ptrace when LSan reports its unsupported tracing environment,
instead of disabling leak checks. No fuzz/test targets are installed in the package.
Finite fuzz runs do not establish absence of bugs or physical hardware correctness.
Use a build filesystem with room; select a separate Cargo target directory when needed.

Report the driver/editor versions, distribution, desktop/session type, profile/mode,
trigger and relevant journal output. Distinguish physical observations from mocks and
from inferred causes. For input regressions, preserve the exact press/rebind/release or
service-transition sequence. Keep installation/migration checks separate from compilation.

Changing Cargo.lock requires regenerating LICENSES/rust in a fresh output directory with
`tools/collect-notices.sh` after dependencies are cached; compare the new notice inventory.
New public releases need a source archive checksum, tested package recipe and corresponding
source/notices for binary redistribution. Do not publish private profiles/session captures.

## Source publication

The public repository is a fresh history rooted at a reviewed tree; private development
history is kept separately and never pushed. Keep it that way: publish subsequent changes
from the public history, never cherry-pick or merge the private one, and compare complete
tree IDs (`git rev-parse 'HEAD^{tree}'`) between the private and public checkouts.

Nothing identifying the owner's machines, accounts or private notes goes into the tree:
no private handles, hostnames, addresses, home or share paths, credentials, profiles,
session captures or maintenance records. Cite a request as "asked <date>" in comments and
commit messages. Site-specific installers, upgrade scripts and validation evidence stay
with the private maintenance notes; the validation record here names machines generically.
Before every publication, grep the delta for known private markers, then read every hunk:
a marker scan does not find what it was not told about.
