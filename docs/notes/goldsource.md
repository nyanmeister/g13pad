# Goldsource

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Native Half-Life and Blue Shift Steam clients (interface 7), 2026-10-09; Counter-Strike: Source 64-bit via the Source module, 2026-10-08.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

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
