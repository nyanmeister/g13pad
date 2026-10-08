# Build profiles and caches

One Cargo workspace and lockfile cover the editor, shared mapping model and analog helper.
All members inherit the workspace release version. The helper and core have no external
crate dependencies; selecting their packages does not compile the GUI stack.
The CLI/watcher selects only the standard library, mapping core and JSON dependencies;
GUI/image/font crates are optional behind the `editor` feature. `g13map edit` replaces
its process with the adjacent `g13map-editor`, preserving the invocation's environment.
Headless text generation and temporary LCD-error rendering also use that helper, without
opening a window. Stored LCD frames/animations, profiles and panel polls stay in the CLI.
Keep both executables together for the full feature set; neither needs hardware or a
display to answer `--version`. The built-in LCD animations live behind the `art` feature
(part of `editor`), shared by the editor's Animations window and the small `g13map-anim`
executable, which needs neither GUI crates nor a display.

```sh
cargo build --locked --release --no-default-features --bin g13map
cargo build --locked --release --bin g13map-editor
cargo build --locked --release --no-default-features --features art --bin g13map-anim
```

Cargo still resolves the entire workspace when validating its lockfile. A first build
on a new machine needs registry index metadata for the editor's dependencies, even when
only the helper is selected. Use `--offline` after that index is populated; compilation
of the helper/core itself requires no external crates.

```sh
cargo build --locked -p g13pad-analog
cargo test --locked -p g13pad-core -p g13pad-analog
cargo test --locked --workspace -- --test-threads=1
cargo build --locked --release --workspace
```

Normal `dev`/`test` builds keep line tables for project backtraces and omit dependency
debug information. For variable inspection and dependency debugging, use the opt-in
profile; its artifacts have their own `full-debug/` subdirectory:

```sh
cargo build --locked --profile full-debug -p g13pad-analog
cargo test --locked --profile full-debug -p g13pad-core -p g13pad-analog
```

Release uses size optimization, thin LTO, one code-generation unit and stripped installed
binaries. LTO costs build time; measure cold and warm builds separately. CMake defaults
to release binaries and ordinary lean test builds. Select another build profile with
`-DG13PAD_RUST_PROFILE=dev` or `full-debug`; select test artifacts with
`-DG13PAD_RUST_TEST_PROFILE=test`, `release` or `full-debug`. The Arch recipe runs tests
in the release profile, reusing compiled dependencies rather than building a second
development dependency tree during packaging.

Keep stable cache paths across source versions, for example:

```sh
G13PAD_BUILD_DIR="$PWD/build-dev" ./build.sh -DG13PAD_RUST_PROFILE=dev
G13PAD_BUILD_DIR="$PWD/build-release" ./build.sh -DCMAKE_BUILD_TYPE=Release
G13PAD_BUILD_DIR="$PWD/build-asan" ./build.sh -DCMAKE_CXX_COMPILER=clang++ \
  -DG13PAD_BUILD_EDITOR=OFF -DG13PAD_BUILD_CLI=OFF -DG13PAD_BUILD_ADAPTER=OFF \
  -DG13PAD_SANITIZER=address,undefined
```

By default each CMake directory keeps Rust artifacts in its `rust/` directory. Explicitly
set `G13PAD_CARGO_TARGET_DIR` for the Arch recipe or `-DG13PAD_CARGO_TARGET_DIR=PATH`
for CMake to share a stable release/package cache when toolchain, features and compiler
flags match. Cargo separates dev, release and full-debug output within a target directory.
Different flags or source-path remapping can legitimately require additional artifacts;
retain packaging reproducibility flags. Avoid moving or clearing a shared cache during
an active build. Existing caches are not cleaned automatically.

Both `build.sh` and the Arch recipe default to two outer and Rust jobs. `G13PAD_JOBS`
sets both; `G13PAD_RUST_JOBS` overrides Rust only. `check.sh` uses the Rust limit saved
by CMake. Choose concurrency from measured memory and responsiveness, rather than CPU
count alone. Use `cargo build --timings` and `/usr/bin/time -v` for actual workflows;
compare cold and warm builds separately. An always-invoked CMake Rust target lets Cargo
check all source/assets for freshness; a no-op invocation does not rebuild dependencies.

Use `cmake --build BUILD --target g13d`, `pbm2lpbm`, `cli`, `editor` or `adapter` to avoid
unrelated components. `-DG13PAD_BUILD_EDITOR=OFF` keeps a small CLI/watcher and the helper;
also disable CLI and adapter for a compiler-only driver environment. A CLI-only install
can apply stored frames but needs the optional editor component to generate new LCD text
or temporary LCD-error frames. Full package staging expects all three Rust binaries.

Source size, executable size, runtime memory and build-cache storage are separate
measurements. Old compiler profiles/features and test binaries can coexist in a cache;
changing release flags does not remove the earlier artifacts. To reclaim a chosen cache,
finish all its active builds first, then use `cargo clean --release --target-dir PATH`
(or omit `--release` to clear all profiles). This deletes generated artifacts and forces
a subsequent cold build; it does not change source or installed programs. A fresh CLI-only
build avoids compiling the editor dependency graph altogether.

## The Source engine health module

`contrib/source-health` builds into `g15-mp-x86_64.so` and `g15-sp-x86_64.so` and, when the
compiler can link `-m32` (Arch: `lib32-glibc` and `lib32-gcc-libs` from multilib), the `i386` pair for
the 32-bit clients such as Half-Life 2; without that toolchain CMake warns and builds
the 64-bit module only. `G13PAD_BUILD_SOURCE_HEALTH=OFF` leaves both out. The tests
`source-health-load*` drive the module through the engine's interface and
`source-health-symbols*` (binutils) refuse libstdc++ and glibc symbols newer than 2.4,
so the module loads inside Steam's runtimes.
