# GoldSource handler (Half-Life and native mods)

The G13 GoldSource handler, `g13map-goldsrc`, launches a native Linux GoldSource game
with a small observer. It
reads the client HUD's `Health` and `Battery` messages and forwards every message
unchanged to the original handler. The feed is `~/.local/state/g13map/health`, shared
with the host even inside Steam Linux Runtime 1.0, with a three-second TTL and a
heartbeat every second. Health is relative to 100; suit armour is the shield bar.
Death sends zero for at least half a second, even if a reset or respawn follows
immediately. Menus and disconnects send `wait`.

## Use

Steam → Half-Life → Properties → Launch options:

```sh
g13map-goldsrc %command%
```

Keep your existing game arguments after `%command%`. A native Decay install uses
the same wrapper with `-game decay`; the observer follows the client library rather
than the game directory's name. A profile in health mode `feed`, with a window rule
for `hl_linux`, shows the meter. `g13map-goldsrc --version` reports the launcher's
version without Steam or a display.

The driver package includes the handler launcher and both native observer
architectures. A standard installation puts the launcher in `/usr/bin`; a custom
prefix can use its launcher path explicitly in Steam's options.

The launcher adds `-insecure`: it is for singleplayer and trusted, insecure co-op
games. It wraps callbacks inside the process; do not use it with secured multiplayer
servers. It does not replace or rename the game's client library. Remove the launcher
from Steam's options to stop loading it; the profile can keep its saved LCD picture
underneath the health-mode tick.

Verified on the installed native Half-Life and Blue Shift Steam clients, 2026-10-09,
on a private X display with copied settings: health, armour, death and disconnect.
Blue Shift also passed through Steam Runtime 1.0's scout-on-soldier container,
using the normal `hl.sh` launch script and the installed observer.
The original 0.2.44 Steam launch hung before opening a game when Steam's overlay
was injected; Half-Life and its hardware meter worked with that preload omitted.
Version 0.2.45 chains into the overlay's unversioned `dlsym` hook, preserving its
initialization and caller context. Shell and both client architectures pass with
the actual overlay. Normal Half-Life and Blue Shift launches through Steam with
the restored overlay preload and hardware health meter were confirmed on
2026-10-09. Blue Shift also passed with the normal launcher rather than the
diagnostic logging wrapper.
Decay's native client has not yet been available for
verification on this machine. Windows DLLs under Wine/Proton need a different loader
and are not supported. Other mods must retain GoldSource interface 7 and the usual
HUD messages; health accepts a byte or signed little-endian short, armour a signed
short. A different health maximum or custom message layout needs a mod-specific
adapter.

## Build and install only this component

Build in a local checkout. Both the 64-bit and 32-bit C++ toolchains are needed for
the launcher: the Steam Half-Life client is 32-bit. CMake's normal build/package
includes the observer; for a small standalone build:

```sh
cmake -S . -B /tmp/g13pad-goldsrc -G Ninja \
  -DG13PAD_BUILD_CLI=OFF -DG13PAD_BUILD_EDITOR=OFF -DG13PAD_BUILD_ADAPTER=OFF \
  -DBUILD_TESTING=ON -DCMAKE_INSTALL_PREFIX="$HOME/.local" -DCMAKE_INSTALL_LIBDIR=lib
cmake --build /tmp/g13pad-goldsrc --target goldsrc-i386 goldsrc-x86_64 \
  fake-goldsrc-i386 fake-goldsrc-x86_64 check-goldsrc-i386 check-goldsrc-x86_64
ctest --test-dir /tmp/g13pad-goldsrc -R goldsrc-health --output-on-failure
cmake --install /tmp/g13pad-goldsrc --component goldsrc-health
```

The launcher uses the dynamic loader's literal `$LIB` token to select an observer
for each executable's architecture. The install supplies Arch and Debian/Steam
directory layouts; an intervening shell or runtime helper does not get the wrong
ELF class. `G13MAP_GOLDSRC_DIR` overrides the module root. `G13MAP_HEALTH_FILE` selects
an absolute feed path for isolated tests; `G13MAP_GOLDSRC_DEBUG=1` logs the client
symbol lookups to stderr.

## Loader details and regression checks

GoldSource loads a client by `dlopen` and resolves its interface with `dlsym`. Steam's
client normally exposes the entire callback table through `F`, bypassing individual
`Initialize`/`HUD_Frame` lookups. The observer handles both routes and changes only
the init, frame and shutdown callbacks. Initialization temporarily wraps the engine's
message-registration slot while the client copies its table, then restores the
engine's slot. The mod's registered handlers are retained and called exactly once.

`tools/check-goldsrc.cpp` drives a separate stand-in `client.so`, so it tests the
actual preload/lookup/callback boundary in both architectures, including malformed
messages, rapid death/reset, heartbeat without new messages, spectator mode and
disconnect. Caller-relative `dlsym(RTLD_NEXT/RTLD_DEFAULT, ...)` calls must tail-call
libc to retain the original caller; keep sibling-call optimization enabled.

The caller-relative lookup regression also loads a library with private scope and
checks that `RTLD_DEFAULT` finds its own symbol and `RTLD_NEXT` finds the next library.
Chained-preload tests add an unversioned loader hook whose constructor requires
its own initialization lookup to reach it. A versioned `dlvsym` alone can bypass
such hooks; use that bootstrap to resolve the next unversioned `dlsym`, then
forward unrelated symbols with tail calls. The original observer fails this
regression during startup. Keep exceptions off in both the observer and stand-in
interposer so their `noexcept` lookup wrappers can tail-call the next function.

The module does not use libstdc++. Explicit old glibc symbol versions plus mandatory
`libdl.so.2`/`librt.so.1` dependencies let it load under scout as well as the host.
Linking plain `-ldl -lrt` on modern glibc can discard those dependencies, which are
still required by old runtimes. `tools/check-goldsrc-symbols.sh` checks both the
symbol versions and dependencies.

Valve's primary ABI/message references:
[APIProxy.h](https://github.com/ValveSoftware/halflife/blob/master/engine/APIProxy.h),
[client exports](https://github.com/ValveSoftware/halflife/blob/master/cl_dll/cdll_int.cpp),
[health](https://github.com/ValveSoftware/halflife/blob/master/cl_dll/health.cpp),
[battery](https://github.com/ValveSoftware/halflife/blob/master/cl_dll/battery.cpp).
