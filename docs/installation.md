# Installation and activation

This package supports Linux x86_64/systemd initially. Builds/staging never install or
activate services. Use an Arch package for upgrades; direct CMake installation does not
preserve existing `/etc/g13` files. Follow migration.md for an existing installation.

## Files and access

`g13.service` runs as the `g13:g13` account, created by systemd-sysusers. Its owned
`/run/g13d` directory is 0750 and FIFOs are 0660. The udev rules grant the account
access to the G13 USB device, uinput, and the source event device. Only intended
controlling users should be members of group `g13`: that group can control the virtual
input device. New group membership takes effect at the next login.

A narrow sudoers rule allows members of that group to start/stop only
`g13-analog.service`. It assumes `/usr/bin/systemctl`, the supported Arch path.
Validate the installed rule with `visudo -cf /etc/sudoers.d/g13pad`.
The service/configuration/helper binaries remain root-owned. No unrestricted sudo rule
or all-input-device group membership is needed.

Systemd runtime, sysusers and module files are included. `/etc/g13/default.bind` is a
neutral boot profile. Keep user bindings in `~/.config/g13map/profiles` and import existing
bindings explicitly rather than overwriting that directory.

## Activate a new installation

Run the root commands using your preferred administrative tool:

```sh
systemd-sysusers
test -c /dev/uinput || modprobe uinput
udevadm control --reload-rules
systemctl daemon-reload
usermod -aG g13 YOUR_LOGIN
```

Optional analog support requires xboxdrv. Append the **owned G13 section** from
`/usr/share/g13pad/g13pad.quirks` to `/etc/libinput/local-overrides.quirks`, preserving
other sections. Avoid duplicate sections. Validate with `libinput quirks validate`.
On Arch, the `libinput` command is supplied by the separate `libinput-tools` package.
Check the validator is available before interrupting an existing installation.
Quirks are a libinput internal API, so retest them after relevant updates.

Log out/in after membership/quirk changes and reconnect the G13 so the udev permissions
apply. Then enable/start `g13.service`. From the user's graphical terminal, import a profile
and apply it:

```sh
g13map import /etc/g13/default.bind default
g13map apply
systemctl --user daemon-reload
systemctl --user import-environment DISPLAY XAUTHORITY
systemctl --user enable --now g13map-apply.service
```

Use `g13map edit` to configure bindings and optional watcher features. Configure profiles
as analog or keys; leave legacy profiles at keep-current when that is intended. The analog
unit is started by that preference rather than enabled unconditionally at boot. Its startup
retries if the source event device has not appeared yet.

The login apply unit runs `g13map detach-pointer` in the user's environment: it detaches
only `pointer:G13` when exactly one source ID is returned. It does not assume a username,
`:0`, or a particular authority file. Native Wayland uses the libinput quirk; X11 needs
xinput if its server has cached the pre-quirk source. Run detach-pointer from that graphical
session before enabling analog on an existing X server.

Both user units start when the user manager does, which on LXQt and i3 is before the
desktop has exported `DISPLAY`, `XAUTHORITY` or `I3SOCK` (they never reach
`graphical-session.target`). So the login apply unit cannot detach the pointer itself; it
reports that, and the watcher does it once the user manager's environment has a display,
for up to five minutes. The watcher finds i3's socket the same way, falling back to
`$XDG_RUNTIME_DIR/i3/ipc-socket.*`; without i3 it opens the X display named there, with
the authority file beside it, so window rules work from the first login on XFCE and the
other X11 desktops too. Nothing assumes `:0`; a session that never exports a display gets
a journal line, not a guess.

## Calibration and troubleshooting

Calibration defaults use the source device's declared range. Copy your own measured
`min centre max` values into `calibration_x`/`calibration_y`; both axes must be supplied
or both omitted. Endpoints must bracket the centre and be within -32768..32767.
The dead zone is 0..32767. Validate before restarting analog:

```sh
g13pad-analog check /etc/g13/analog.conf
```

Check `journalctl -u g13.service -u g13-analog.service` and
`journalctl --user -u g13map-watch.service` on failures. A stale FIFO without a reader is
not a working daemon. Profile stick transitions require group membership and the narrow
rule; failures leave the previous active profile selected. Preserve the current profile
and calibration during recovery.

The driver service initializes `/run/g13d/analog.map` as a regular `g13:g13` file with
mode 0660; the analog helper reads it under a shared lock. The editor validates and locks
the writable file before stopping an adapter, writes the selected profile's mapping, and
restarts it. Failed transitions restore the previous mapping and adapter state where
possible, with explicit recovery errors if rollback fails. Custom mappings require the
updated helper and both system units; an older installation can still use the defaults.

Profiles store mappings as `# gamepad STICK CLICK SWAP INVERT_X INVERT_Y`, for example
`# gamepad right r3 1 0 1`. Stick is `left` or `right`; click is `a b x y lb rb back start
guide l3 r3 none`; the three flags are `0` or `1`. Missing metadata uses left/L3/no swap,
physical X unchanged and physical Y inverted, preserving the previous behavior. These
comments are editor metadata, not commands sent to g13d.

Keep `g13map-watch.service` running for LCD error restoration, including static profiles
and configurations without M-key/i3 switching. The editor starts it when needed. Errors
temporarily take priority over normal LCD writes; normal requests continue updating the
restoration cache. Identical active errors do not extend their five-second expiry. Disable
the feature using the editor's checkbox (stored in `~/.config/g13map/lcd-errors`). A
disconnected physical LCD cannot show an error; use the journal for driver failures.

## Panel applets

For LXQt, the existing custom-command plugin can run `g13map` (or `g13map panel lxqt`)
with click action `g13map edit`, structured output `outputFormat=2`, and a 30-second repeat
timer. Its icon, profile label and tooltip retain the existing format.

For XFCE, install the optional `xfce4-genmon-plugin`, add a **Generic Monitor** panel
item, and use `g13map panel xfce` as its command. Hide the plugin's extra label and set
the update interval to 30 seconds. Both the device icon and profile text open the same binary's
editor; grey icon and a cross indicate an unavailable driver. The tooltip names the active
profile and explains the state. No background process, service calls, or device writes are
needed for these status polls.

The package installs connected/disconnected SVG icons in the hicolor theme. For a custom
installation prefix, include its share directory in `XDG_DATA_DIRS` so XFCE can find them.
Genmon 4.2 and newer store settings in xfconf, not the old `genmon-ID.rc` files. Its current
properties under `/plugins/plugin-ID` are `command` (string), `use-label` (bool),
`update-period` (int, milliseconds: 30000), and `enable-single-row` (bool). An immediate
refresh uses `xfce4-panel --plugin-event=genmon-ID:refresh:bool:true`. The native output
tags and current settings are documented by
[Xfce](https://docs.xfce.org/panel-plugins/xfce4-genmon-plugin/start).

Package installation does not rewrite either desktop's panel configuration or require
either panel to use the editor.

Both adapters use the same device mark, cyan/grey connection state, active profile and
tooltip. The mark depicts the LCD, key matrix, offset thumbstick and palm rest. Desktop
fonts and icon sizing follow each panel's theme. No custom LXQt plugin is necessary:
use its standard **Custom Command** plugin with structured output. Panel widths should
accommodate your profile names rather than clipping them to a fixed label length.

For Waybar, add a custom module (JSON format, interval in seconds):

```json
"custom/g13pad": {
  "exec": "g13map panel waybar",
  "return-type": "json",
  "interval": 30,
  "on-click": "g13map edit"
}
```

Add `custom/g13pad` to a modules list. The JSON includes `connected`/`disconnected` CSS
classes; choose colours to match your bar. This adapter has format/escaping checks;
native Waybar rendering has not been tested. Window rules follow i3, sway or any X11 window
manager, and physical keyboard-label translation targets X11; panel support does not extend
those features.
For Polybar, i3blocks or tint2 command items, use `g13map panel text`, a 30-second poll and
`g13map edit` as the click action where supported. That adapter reports the same profile
and connection in plain text; these bars do not share LXQt/XFCE's native icon/tooltip contract.
KDE, GNOME and Cinnamon can use the installed **G13 Pad** application launcher and full CLI.
A native tray/extension for those desktops is not currently shipped.
