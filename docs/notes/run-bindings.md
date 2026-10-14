# A program on a key

Written 2026-10-13 by Claude (Fable 5.1) for 0.2.98, asked that day: "Shell script on
button press ... launch my web-browser from the pad, or a terminal emulator with a command
that'll run inside it." Hobbyist add-on by design: the script comes from anywhere on the
computer; shipping some under `/usr/share/g13pad/scripts/` is allowed, like the animations.

## The shape

- Two lines in the profile: `bind KEY >RUN:KEY;` for the daemon and `# run KEY PROGRAM
  [ARGS...]` for the watcher. The driver's pipe-out action (`>TOKEN;`, `driver/g13_action.cpp`)
  writes the token on press; `g13map watch` already read that pipe for `AUDIO:` and `MPD:`
  tokens, and `RUN:` is the third family (`modes.rs`, before the M-Sum check so it works
  with modes off). `run.rs` holds the grammar, the lookup and the start.
- **The token carries the key, never the command.** `/run/g13d/g13-0_out` is group-writable
  and the watcher drops anything past 64 bytes as "not ours"; a command line in the token
  would be code execution for any writer of the pipe. A profile can already type any
  keystroke sequence, so a path in it adds no trust that was not there.
- A word with `/` is a path (`~` is home); a bare name is looked up in
  `~/.config/g13map/scripts/`, then `/usr/share/g13pad/scripts/`, then `PATH`. The program
  is exec'd directly (no `sh -c`): a script needs `#!` and the executable bit, and a path
  with spaces needs a wrapper. Set-time check in the CLI and the editor: it must resolve.
- Started through `systemd-run --user --quiet --collect`, with `G13MAP_KEY`, `G13MAP_PROFILE`
  and the watcher's `DISPLAY`/`WAYLAND_DISPLAY`/`XAUTHORITY`/`DBUS_SESSION_BUS_ADDRESS`.
  Reason: a program spawned as a child lives in the watcher's cgroup, and `systemctl --user
  restart g13map-watch.service` (every package install) would kill the browser the user
  started from the pad. A transient unit also puts the program's output in the journal
  under its own name. Fallback when there is no manager (`G13MAP_UNIT=0`, a TTY): a child
  with a reaper thread. Every press starts the program; a script that must not run twice
  guards itself with `flock`.
- The parser notes a bind without its `# run` line and a `# run` line without its bind;
  the CLI refuses edits until repaired, as with any other malformed line. `profile bind`
  to another action and `profile unbind` drop the `# run` line; the editor does the same
  in `set_action`.

## Release actions and hold (0.3.0)

Asked 2026-10-14, picked from a mockup sheet (private maintenance notes, mockup-20261014): one program,
told which edge, with hold as a fourth choice. The pane's **When** row: Pressed, Released,
Both, Hold; hovering a radio gives the detail.

- **The driver reports both ends when asked.** `G13_Action_PipeOut` takes two
  space-separated groups like the key action: `bind G5 >RUN:G5; RELEASE:G5;` writes
  `RUN:G5;` on press and `RELEASE:G5;` on release; `>RUN:G5;` alone stays press-only, so
  every existing profile, the M-key routes and the audio and music tokens are untouched.
  Proof in `tools/driver-audit.cpp` (`check_pipe_out_release`, hand-derived bytes).
- **The line says which ends matter.** `# run KEY [pressed|released|both|hold] PROGRAM
  [ARGS...]`; the word is never written for `pressed`, so old files read and write the
  same. `run::Run { when, program }` is the parsed shape; `run::binding(key, when)` picks
  the bind. A profile whose mode needs the release but whose bind lacks the second group
  gets a note like any other half-binding. A program that is itself named like a mode word
  is given as a path (`./both`).
- **The watcher** (`modes.rs`) matches `(when, edge)`: Pressed ignores the release token,
  Released ignores the press, Both starts on each with `G13MAP_EDGE=press|release` (set on
  every start, Pressed included), Hold goes to `run::Holds`.
- **Hold** starts the program as the transient unit `g13map-hold-KEY` (`systemd-run
  --unit=`), so the release is `systemctl --user stop g13map-hold-KEY`, which is SIGTERM
  to the program and it gets to finish its file. A second press while the unit still runs
  is refused ("still running"), not a second copy. Without a manager (`G13MAP_UNIT=0`, a
  TTY) the child is kept by key and sent SIGTERM directly; `Child::kill` would be SIGKILL.
- CLI: `g13map profile run NAME KEY [WHEN] PROGRAM [ARGS...]`.

Checking it: `run.rs` tests the grammar both ways, both bind shapes (and that the two
halves must name the same key), the token for each edge, `G13MAP_EDGE` in a started
script's output (`G5 gaming press hello` / `... release hello`), and a held script that
appends `started` on start and `stopped` from its TERM trap: press, refused second press,
release, both words in order, then a fresh press after it ended. The boundary is the VM
with the pad: a `hold` profile through the installed package, the unit visible under
`systemctl --user list-units 'g13map-hold-*'` while the key is down and gone after.

## Checking it

- Unit tests in `run.rs`: tokens and bindings; words; resolution order with a user `sh`
  shadowing PATH's, a non-executable file and a missing path refused with the reason; a
  started script writing `$G13MAP_KEY $G13MAP_PROFILE $1` to `$2` (expected `G5 gaming
  hello`, broken once by renaming the variable: the test failed on the content). `cli.rs`
  and `profile.rs` cover the two-line save, the drops and the notes.
- The boundary is the VM: a profile with `# run G5 .../mark.sh` and `bind G5 >RUN:G5;`
  applied through the installed package, the key pressed on the pad, the watcher's journal
  line `run G5: ... (a unit of the session)`, the marker file written, and the transient
  unit visible under `systemctl --user list-units 'run-*'` while a long program runs.
