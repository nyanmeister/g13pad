# G13 USB passthrough to Arch-Virtual

These optional files are for the libvirt **host**. They are not installed by the desktop
package. The policy targets one Logitech G13 (046d:c21c) and the VM `Arch-Virtual`.
The host needs libvirt's `virsh`, `xmllint` (libxml2) and `flock` (util-linux).

Libvirt's persistent vendor/product selection discovers the current device on VM startup.
`startupPolicy="optional"` permits startup without the pad. A physical reconnect while the
VM remains running can leave the live hostdev at its old bus/device address. The udev
rule starts an independent systemd oneshot to compare the new sysfs address with that
live attachment, detach only a stale G13 entry, and attach the current pad. A matching
attachment is a no-op; a stopped VM is left stopped. Multiple matching entries are rejected.
The helper serializes events and propagates libvirt errors. It never changes the persistent
domain or starts/restarts a VM. This policy assumes only one physical G13.

Do not call virsh from a synchronous QEMU hook: libvirt documents that this can deadlock.
Use systemd outside the hook execution instead.
References: [libvirt USB hostdev](https://libvirt.org/formatdomain.html#usb-pci-scsi-devices),
[libvirt hook restrictions](https://libvirt.org/hooks.html#calling-libvirt-functions-from-within-a-hook-script),
[systemd device activation](https://www.freedesktop.org/software/systemd/man/latest/systemd.device.html).

On the host, back up the active and persistent domain XML before making changes. Install
the files from `packaging/vm/` as root:

```sh
install -Dm755 g13pad-usb-reattach /usr/local/libexec/g13pad-usb-reattach
install -Dm644 g13pad-usb-reattach@.service /etc/systemd/system/g13pad-usb-reattach@.service
install -Dm644 71-g13pad-vm.rules /etc/udev/rules.d/71-g13pad-vm.rules
install -Dm644 g13pad-Arch-Virtual.xml /etc/libvirt/g13pad-Arch-Virtual.xml
systemctl daemon-reload
udevadm control --reload-rules
```

In the persistent domain, replace the existing G13 hostdev with the supplied fragment
(retain its guest USB address if present). Do not append a duplicate device. Review and
validate the complete domain before `virsh define`. If no G13 entry exists yet, use
`virsh -c qemu:///system attach-device Arch-Virtual /etc/libvirt/g13pad-Arch-Virtual.xml --config`.
No libvirt daemon restart is needed. On an already plugged device, activate once using
`systemctl start g13pad-usb-reattach@4-1.3.service`, substituting the actual sysfs name.
Device add events after a removal will activate it automatically. The oneshot does not
remain active after completing.

Check `journalctl -u 'g13pad-usb-reattach@*'`, live `virsh dumpxml`, guest USB visibility,
driver/adapter/watcher health and saved configuration hashes. Exercise an actual replug
when convenient; a synthetic udev event establishes rule matching, not physical reconnect.
`tools/check-vm-usb.sh` tests the policy using private sysfs/XML and a fake virsh.
`g13pad-usb-reattach --version` requires neither root, hardware nor libvirt.

To remove automatic reconnect, delete the installed rule, unit and helper and reload
udev/systemd. The optional persistent hostdev can remain for startup-only passthrough.
To restore its original startup policy, remove just that source attribute. To return the
pad to the host, detach the matching G13 device with `--live --config`; removal of the
udev rule prevents a later replug from handing it back. Preserve later unrelated VM changes
instead of blindly restoring an old whole-domain XML backup.
