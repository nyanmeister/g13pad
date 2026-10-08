#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Builds only; installation and activation are separate operations.
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
build_dir=${G13PAD_BUILD_DIR:-"$repo/build"}
cmake -S "$repo" -B "$build_dir" -G Ninja \
  -DG13PAD_RUST_JOBS="${G13PAD_RUST_JOBS:-${G13PAD_JOBS:-2}}" "$@"
cmake --build "$build_dir" -j "${G13PAD_JOBS:-2}"
