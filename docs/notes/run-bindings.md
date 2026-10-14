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

## Not done, and how it would be

- **Release actions.** `G13_Action_PipeOut::act` writes only on `is_down`. The key action
  beside it takes two space-separated groups, press and release; giving pipe-out the same
  (split the text on the first space in its constructor, write the second on release) is
  about ten lines of C++ and a driver rebuild, and a `# release KEY PROGRAM` line in the
  profile. A single token keeps firing on press only, so the M-key routes and the audio and
  music bindings are untouched. On the radar (asked 2026-10-13), not in 0.2.98.

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
