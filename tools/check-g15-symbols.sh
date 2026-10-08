#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# The Source LCD module loads inside Steam's runtimes: no libstdc++, no glibc symbol
# version newer than 2.4, and CreateInterface exported.
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' 'g13pad-g15-symbols 0.2.32'; exit 0; fi
module=$1
command -v objdump >/dev/null 2>&1 || { echo "objdump missing; skipped"; exit 77; }
command -v readelf >/dev/null 2>&1 || { echo "readelf missing; skipped"; exit 77; }
status=0
needed=$(readelf -d "$module" | awk '/NEEDED/ {print $NF}')
printf 'NEEDED: %s\n' "$needed" | tr '\n' ' '; echo
case "$needed" in
  *stdc++*) echo "FAIL: links libstdc++"; status=1 ;;
esac
newest=$(objdump -T "$module" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)
echo "newest glibc symbol version: ${newest:-none}"
case "$newest" in
  ""|GLIBC_2.[0-4]|GLIBC_2.[0-4].*) ;;
  *) echo "FAIL: $newest is newer than the Steam runtimes allow"; status=1 ;;
esac
objdump -T "$module" | grep -q ' CreateInterface$' || { echo "FAIL: CreateInterface not exported"; status=1; }
objdump -T "$module" | grep -q ' g13pad_g15_version$' || { echo "FAIL: no version string"; status=1; }
[ "$status" -eq 0 ] && echo "symbols ok"
exit $status
