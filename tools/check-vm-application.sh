#!/usr/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Real USB integration in the authorized test VM; no generated desktop input.
set -Eeuo pipefail
if [[ ${1:-} == --version ]]; then echo 'g13pad-vm-application-check 0.2.8'; exit 0; fi
# G13PAD_VM_HOST names the authorized test VM; the check refuses to run anywhere else.
[[ -n ${G13PAD_VM_HOST:-} && $(hostname) == "$G13PAD_VM_HOST" && $# == 1 && $(id -u) != 0 ]] || exit 2
out=$(realpath -e "$1")
[[ -z ${G13MAP_CONFIG:-} && -z ${G13MAP_UNIT:-} && -z ${G13MAP_ANALOG:-} ]] || exit 2
saved_config=$HOME/.config/g13map
original=$(cat "$saved_config/active")
# This fixture restores the original analog profile, including its runtime mapping.
grep -q '^# stick analog$' "$saved_config/profiles/$original.bind"
find "$saved_config" -type f -print0 | sort -z | xargs -0 sha256sum >"$out/saved.sha256"
sha256sum /etc/g13/default.bind /etc/g13/analog.conf /run/g13d/analog.map >"$out/runtime.sha256"
fixture=$(mktemp -d)
watch_pid=''
cleanup() {
    result=$?
    trap - EXIT
    set +e
    if [[ -n $watch_pid ]]; then kill "$watch_pid"; wait "$watch_pid"; fi
    unset G13MAP_CONFIG
    g13map apply >"$out/restore.log" 2>&1 || result=1
    systemctl --user start g13map-watch.service || result=1
    sha256sum -c "$out/saved.sha256" || result=1
    sha256sum -c "$out/runtime.sha256" || result=1
    rm -rf "$fixture"
    exit "$result"
}
trap cleanup EXIT
systemctl --user stop g13map-watch.service
cp -a "$saved_config/." "$fixture/"
mkdir -p "$fixture/profiles"
printf '# stick analog\n# gamepad left l3 0 0 1\nmod 3\nbind G1 KEY_A\n' >"$fixture/profiles/test-left.bind"
printf '# stick analog\n# gamepad right r3 1 1 0\nmod 2\nbind G2 KEY_B\n' >"$fixture/profiles/test-right.bind"
printf '# stick keys\nmod 1\nbind TOP KEY_C\n' >"$fixture/profiles/test-keys.bind"
printf 'on\n0 test-left\n1 test-right\n2 test-keys\n' >"$fixture/modes"
printf 'off\n' >"$fixture/focus"
printf 'test-left\n' >"$fixture/active"
export G13MAP_CONFIG="$fixture"
g13map apply
systemctl is-active --quiet g13-analog.service
inspect() {
    pid=$(systemctl show g13-analog.service -p MainPID --value)
    tr '\0' ' ' <"/proc/$pid/cmdline"
    echo
}
inspect >"$out/left-argv.txt"
grep -q 'ABS_X=x1,ABS_Y=y1' "$out/left-argv.txt"
grep -q 'BTN_EXTRA=TL' "$out/left-argv.txt"
g13map watch >"$out/watch.log" 2>&1 &
watch_pid=$!
for _attempt in $(seq 1 50); do
    kill -0 "$watch_pid"
    if grep -q 'modes on' "$out/watch.log"; then break; fi
    sleep .1
 done
grep -q 'modes on' "$out/watch.log"
await_profile() {
    expected=$1
    for _attempt in $(seq 1 100); do
        kill -0 "$watch_pid"
        if [[ $(cat "$fixture/active") == "$expected" ]]; then return 0; fi
        sleep .1
    done
    echo "profile did not become $expected" >&2
    return 1
}
# The child shell expands its positional parameter, not this parent.
# shellcheck disable=SC2016
signal_mode() { timeout 3 bash -c 'printf "%s;" "$1" > /run/g13d/g13-0_out' _ "$1"; }
signal_mode M1
await_profile test-right
inspect >"$out/right-argv.txt"
grep -q 'ABS_X=y2,ABS_Y=x2' "$out/right-argv.txt"
grep -q 'BTN_EXTRA=TR' "$out/right-argv.txt"
signal_mode MR
await_profile test-left
signal_mode M2
await_profile test-keys
[[ $(systemctl show g13-analog.service -p ActiveState --value) == inactive ]]
signal_mode MR
await_profile test-left
systemctl is-active --quiet g13-analog.service
# Reconnection runs the same shared application path from the watcher.
sudo -n systemctl restart g13.service
for _attempt in $(seq 1 100); do
    if systemctl is-active --quiet g13-analog.service && grep -q "driver reconnected: restored 'test-left'" "$out/watch.log"; then break; fi
    sleep .1
 done
grep -q "driver reconnected: restored 'test-left'" "$out/watch.log"
systemctl is-active g13.service g13-analog.service
inspect >"$out/recovered-argv.txt"
grep -q 'ABS_X=x1,ABS_Y=y1' "$out/recovered-argv.txt"
cp "$fixture/active" "$out/fixture-active"
echo 'PASS: installed CLI apply, watcher M1/MR/M2 transitions and driver restart recovery'
