#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
if [ "${1:-}" = --version ]; then echo 'g13pad-vm-usb-check 0.2.8'; exit 0; fi
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$work/bin" "$work/sys/4-1.3"
export G13PAD_USB_SYSFS="$work/sys" G13PAD_USB_LOCK="$work/lock"
export G13PAD_USB_XML="$repo/packaging/vm/g13pad-Arch-Virtual.xml" G13PAD_USB_TEST="$work"
printf '046d\n' >"$work/sys/4-1.3/idVendor"
printf 'c21c\n' >"$work/sys/4-1.3/idProduct"
printf '4\n' >"$work/sys/4-1.3/busnum"
printf '4\n' >"$work/sys/4-1.3/devnum"
cat >"$work/bin/virsh" <<'SH'
#!/bin/sh
set -eu
shift 2
case "$1" in
 domstate) cat "$G13PAD_USB_TEST/state" ;;
 dumpxml) cat "$G13PAD_USB_TEST/live.xml" ;;
 detach-device|attach-device)
   echo "$1" >>"$G13PAD_USB_TEST/actions"
   xmllint --noout "$3"
   [ ! -e "$G13PAD_USB_TEST/fail" ] ;;
 *) exit 2 ;;
esac
SH
chmod +x "$work/bin/virsh"
export PATH="$work/bin:$PATH"
run() { sh "$repo/packaging/vm/g13pad-usb-reattach" 4-1.3; }
live() {
 printf '<domain><devices><hostdev type="usb"><source><vendor id="0x046d"/><product id="0xc21c"/><address bus="4" device="%s"/></source></hostdev></devices></domain>\n' "$1" >"$work/live.xml"
}
printf 'running\n' >"$work/state"
live 3
run
printf 'detach-device\nattach-device\n' >"$work/expected"
cmp "$work/actions" "$work/expected"
rm "$work/actions"
live 4
run
[ ! -e "$work/actions" ]
echo '<domain><devices/></domain>' >"$work/live.xml"
run
[ "$(cat "$work/actions")" = attach-device ]
rm "$work/actions"
printf 'shut off\n' >"$work/state"
run
[ ! -e "$work/actions" ]
printf 'running\n' >"$work/state"
printf '0000\n' >"$work/sys/4-1.3/idProduct"
run
[ ! -e "$work/actions" ]
printf 'c21c\n' >"$work/sys/4-1.3/idProduct"
live 3
touch "$work/fail"
if run; then echo 'detach failure was ignored' >&2; exit 1; fi
[ "$(cat "$work/actions")" = detach-device ]
if sh "$repo/packaging/vm/g13pad-usb-reattach" ../escape; then exit 1; fi
echo 'PASS: stale/current/absent/off/wrong-device/detach-failure/invalid-name'
