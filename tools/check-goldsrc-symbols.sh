#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
if [ "${1:-}" = --version ]; then echo 'g13pad-goldsrc-symbols 1'; exit 0; fi
module=$1
needed=$(readelf -d "$module" | awk '/NEEDED/ {print $NF}')
case "$needed" in *stdc++*) echo 'FAIL: links libstdc++'; exit 1;; esac
printf '%s\n' "$needed" | grep -q 'libdl.so.2' || { echo 'FAIL: old runtime needs libdl'; exit 1; }
printf '%s\n' "$needed" | grep -q 'librt.so.1' || { echo 'FAIL: old runtime needs librt'; exit 1; }
newest=$(objdump -T "$module" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)
case "$newest" in ""|GLIBC_2.[0-4]|GLIBC_2.[0-4].*) ;; *) echo "FAIL: $newest too new for scout"; exit 1;; esac
objdump -T "$module" | grep -q ' dlsym$'
objdump -T "$module" | grep -q ' g13pad_goldsrc_version$'
echo "GoldSource symbols passed ($newest)"
