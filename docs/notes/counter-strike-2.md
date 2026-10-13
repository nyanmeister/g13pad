# Counter strike 2

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Counter-Strike 2 Game State Integration, 2026-10-07.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Games that talk on their own (Counter-Strike 2)

CS2's Game State Integration posts JSON to a local HTTP listener named in a cfg file in
`game/csgo/cfg/`; `g13map health cs2-config` prints it. A profile in health mode `cs2`
makes the watcher run the listener itself while the profile is active, so
nothing has to be started by hand. `player.state.health`, `armor` and `helmet` are the
fields; while dead, `player` is whoever is being spectated (compare `player.steamid`
with `provider.steamid`), and the menu has no `player.state` at all. Counter-Strike:
Source has the same armour and helmet but no Game State Integration; it needs another
route (a server plugin), and a profile in mode `cs2` there waits forever.

The CS2 listener uses a loopback TCP socket, not a network service reachable from
other machines. `meter::tests::profile_names_select_the_meter` starts and drops the
listener, then binds its port to verify that dropping it released the socket.
Some agent sandboxes deny even `TcpListener::bind` on `127.0.0.1`; this produces
`Operation not permitted` before any game traffic or external connection. Re-run
the affected check with authorized localhost/socket access rather than changing
the listener or treating the permission failure as a regression. A repository
note explains the failure; it cannot grant sandbox permissions. Verified 2026-10-09:
the same workspace suite passed with localhost access after this sandbox denial.
