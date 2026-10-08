#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify two opposite pixels plus strict rejection of wrong-size data.
set -eu
binary=${1:?converter path required}
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
printf 'P4\n# fixture\n160 43\n\200' > "$fixture/input"
dd if=/dev/zero bs=1 count=858 status=none >> "$fixture/input"
printf '\001' >> "$fixture/input"
printf '\001' > "$fixture/expected"
dd if=/dev/zero bs=1 count=958 status=none >> "$fixture/expected"
printf '\004' >> "$fixture/expected"
"$binary" < "$fixture/input" > "$fixture/output"
cmp "$fixture/output" "$fixture/expected"
printf '\000' >> "$fixture/input"
if "$binary" < "$fixture/input" > "$fixture/output" 2>/dev/null; then
  echo 'Oversized input was accepted' >&2; exit 1
fi
test ! -s "$fixture/output"
printf 'P4\n160 43\n\000' > "$fixture/input"
if "$binary" < "$fixture/input" > "$fixture/output" 2>/dev/null; then
  echo 'Truncated input was accepted' >&2; exit 1
fi
test ! -s "$fixture/output"
