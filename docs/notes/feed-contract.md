# Feed contract

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## Who this is for

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

## Choosing where it shows

The meter is a profile's display choice (`Display::Health(reader)`; `g13map profile health NAME
feed|cs2|log`; the picture stays kept underneath), so the window rules put it on the
game's windows and nowhere else. Two profiles with the same reader
hand the meter across a switch; any other picture ends it, and the ending profile's
own backlight comes back (the meter was once restoring the colour of the profile it
started under, over the new one: if a colour "leaks" between profiles, look there).
