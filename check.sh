#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu
repo=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
build_dir=${G13PAD_BUILD_DIR:-"$repo/build"}
cmake --build "$build_dir" -j "${G13PAD_JOBS:-2}"
ctest --test-dir "$build_dir" --output-on-failure
