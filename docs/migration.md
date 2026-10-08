# Migration from the private patched installation

The consolidated source is a successor to g13map plus its four driver patches. The
existing installed editor/driver and profile directory continue to work until migration.
Consolidation alone does not install anything. The historical root scripts for the old
installation stay with the owner's private maintenance notes, not in this tree.

Before replacing a running setup, record the active profile, enabled/active units, both
installed binary hashes and unit/drop-in contents. Save `/etc/g13/default.bind`, measured
calibration, root units/drop-ins/permissions/quirks/sudoers and the user's profiles/units.
Do not restore a stale snapshot over newer profile edits. Authenticate before interrupting
services, and have a rollback copy that restores the immediately prior binaries/units.

For the known private setup, conflicts to review are:

- `/etc/systemd/system/g13-analog.service`, which overrides the packaged unit and embeds
  the old username/display/calibration; move its measurements into analog.conf.
- `g13.service.d/90-input-safety.conf`, which pins the old driver path, and `analog.conf`,
  which starts analog unconditionally. Retain backups, then retire these when adopting
  the package's driver and per-profile analog lifecycle.
- Older user units in `~/.config/systemd/user` and `~/.local/bin/g13map`, which override
  packaged units/binaries. Retire the units and old command only after verifying new paths.
- Old USB/analog udev rules and sudoers entries. Replace the owned rules, preserve unrelated
  files, and verify access and sudoers before enabling new mode changes.
- The libinput G13 quirk: preserve an existing matching section and all other local quirks.
  Do not blindly replace the complete overrides file.

Use the staged manifest to inspect every installed path. The Arch package conflicts with
`g13-git` because both supply g13d and g13.service; use a planned package replacement,
not two competing drivers. Pacman's backup declarations preserve edited startup/config
files, but files owned by the old package and private overrides still require this review.

Inspect the old package's removal script before replacing it. The migrated
`g13-git v1.0.4.r13.g1e80eda-1` script deletes the service account and changes its groups.
The development desktop's verified replacement and rollback used pacman's `--noscriptlet` option
and explicitly preserved the account, adopted the new units/rules, and reloaded systemd
and udev. Those takeover steps are required when skipping scripts; this is not a general
recommendation to skip unrelated package scripts.

Stop the watcher before replacing the driver, with root authentication already complete.
After package/configuration adoption and a fresh login/device permission refresh, start
the driver, reapply the saved profile, and restore the watcher according to the user's
previous preferences. Check that all three components recover and that each running
process uses the expected binary. User-assisted checks: keyboard remains attached, source
pointer stays still, stick-click/directions/analog mode work, LCD/profile follow correctly,
then fresh login and physical reconnect. Automated mocks do not replace these checks.

Rollback restores the immediately previous unit/drop-in configuration and binaries,
reloads units/rules, starts the old driver, reapplies the saved active profile, and restores
its watcher state. Keep the tested custom driver on rollback; do not accidentally fall
back to stock 1e80eda's held-key/mode bugs. The development machine's existing rollback
records remain in its G13 Review runbooks. Initial source consolidation did not install
anything; the subsequent development-desktop migration is recorded below.

## Development desktop status — 2026-10-02

The consolidated package is installed, currently version 0.2.9. Comparison against the
installed 0.2.3 used copied saved profiles, a private X display and simulated FIFOs. The
upgrade preserved startup bindings, measured calibration, all saved profile/LCD/preference
files, runtime mapping and panel configuration. Installed CLI/editor and native LXQt
checks passed; running driver/watcher hashes match installed files. The physical pad is
still routed to the server VM, so the desktop's driver/watcher wait for it and analog remains
stopped. Earlier mapping/LCD/reconnect observations are historical; no new physical main-
machine observation is implied. See [validation](validation.md) for exact boundaries.

During the initial migration, the desktop predated new `g13` group membership. A private
helper temporarily granted runtime ACL access and the same two exact adapter commands.
That bridge was removed after actual user-manager and i3 process groups were verified;
normal packaged access is now used. The migration script and the reviewed
0.2.3-to-0.2.9 upgrade script (root authentication before service interruption, fresh
snapshots, package recovery) are site-specific, not general installers, and are kept
with the owner's private maintenance notes together with the immediate old package,
root/user snapshots, original service policy and selective rollback.
Do not run the initial migration installer or restore stale profiles over later edits.
