# Controller bindings

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## Controller bindings

See [controller bindings](docs/controller-bindings.md). `G13_PAD_SEQ` and the
core's `gamepad::BUTTONS` share the fixed private channel order. TOP uses the
same refcounted channel as board buttons; mapping its old BTN_EXTRA to the same
xboxdrv output would let either release cancel the other held button. Disabling
TOP must not disable G-key L3. Keep modern west/north X/Y output codes explicit:
xboxdrv's historical aliases and jstest's printed names use the old convention.
The native `tools/controller-proof.rs` checks real xboxdrv in the isolated VM;
driver action proofs check shared holds and releases across rebindings.

## The analog service does not survive a G13 reconnect (found 2026-10-14)

xboxdrv 0.8.15 reads the source with a glib watch whose callback returns `TRUE` no
matter what `read()` returned. When the pad drops off USB, g13d reopens it in-process and
recreates its uinput device, udev points `/dev/input/g13-analog-source` at the new node,
and xboxdrv keeps fd 8 on the deleted one: `ppoll` wakes at once, `read` fails with
`ENODEV`, and it loops about 9,400 times a second (one core, two thirds kernel time) while
the stick no longer reaches the Xbox device. Seen on the owner's desktop after the 21:25
reconnect on 2026-10-13; `systemctl restart g13-analog.service` cured it. Diagnosis that
needed root: `ls -l /proc/<pid>/fd` showed `event10 (deleted)`, `strace -c` showed the
loop. Not fixed in code yet. Two candidate fixes, pick one for 0.3.0:
- udev: `TAG+="systemd", ENV{SYSTEMD_WANTS}="g13-analog.service"` on the source rule, and
  `BindsTo=dev-input-g13\x2danalog\x2dsource.device` in the unit, so systemd stops the
  service with the node and starts it again on the next add. No code; verify that
  systemd makes a device unit for the symlink (it does for `by-id` links).
- `g13pad-analog run` spawns xboxdrv instead of exec'ing it, stats the source path once a
  second, and exits non-zero when the inode changes so `Restart=on-failure` brings the
  unit back through `await` and `prepare`.

Decision 2026-10-14 (asked: keep Linux only, live with the systemd dependency): the udev
route. `packaging/70-g13pad.rules` tags the source `systemd`, `g13-analog.service` has
`BindsTo=`/`After=` on `dev-input-g13\x2danalog\x2dsource.device`, and the watcher's
existing reconnect restore (`switch_to` on a new driver pipe) starts the adapter again when
the profile wants it; `packaging/g13pad.install` re-runs the rule on an already plugged pad
so the device unit exists right after the upgrade. `g13map doctor` reports a stale adapter
(started before the current symlink) without root. The fold-into-g13d route stays on the
table if a second platform ever matters.
