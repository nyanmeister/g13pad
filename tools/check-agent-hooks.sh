#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Black-box hook contract. Uses a private runtime, never a watcher or the G13.
set -euo pipefail
repo=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
bin=$(realpath "${1:-$repo/target/release/g13map}")
scratch=$(mktemp -d /tmp/g13-agent-check.XXXXXX)
trap 'rm -rf -- "$scratch"' EXIT
export XDG_RUNTIME_DIR="$scratch/runtime" G13MAP_CONFIG="$scratch/config"
export G13MAP_PIPE="$scratch/pipe" G13MAP_OUT_PIPE="$scratch/out"
export G13MAP_UNIT=0 G13MAP_ANALOG=1 G13MAP_ANALOG_MAP="$scratch/analog.map"
export WINDOWID=424242
feed="$XDG_RUNTIME_DIR/g13map-pipe.agents/$WINDOWID"
"$bin" --version
hook() {
  local hook_output="$scratch/stdout-$BASHPID"
  printf '%s\n' "${2:-null}" | "$bin" agent codex "$1" > "$hook_output"
  test ! -s "$hook_output"
}
hook UserPromptSubmit
read -r phase activity key started rest < "$feed"
test "$phase $activity $key" = 'turn THINKING started'
hook PermissionRequest
test "$(cat "$feed")" = "review started $started ttl 21600"
hook HumanPermissionRequest
test "$(cat "$feed")" = "ask started $started ttl 21600"
hook PreToolUse '{"tool_name":"apply_patch"}'
test "$(cat "$feed")" = "turn EDITING started $started ttl 900"
# Concurrent events must not lose increments or collide on the .new file.
pids=()
for ((i=0; i<40; i++)); do hook SubagentStart & pids+=("$!"); done
for pid in "${pids[@]}"; do wait "$pid"; done
test "$(cat "$feed")" = "turn EDITING started $started agents 40 ttl 900"
hook Stop
test "$(awk '{print $1}' "$feed")" = ready
for ((i=0; i<40; i++)); do hook SubagentStop; done
test "$(awk '{print $1}' "$feed")" = ready
if rg -q 'agents' "$feed"; then
  echo 'FAIL: completed subagents still counted' >&2
  exit 1
fi
before=$(cat "$feed")
hook FutureEvent
test "$(cat "$feed")" = "$before"
printf 'null\n' | env -u WINDOWID "$bin" agent codex UserPromptSubmit > "$scratch/no-window"
test ! -s "$scratch/no-window"
test "$(cat "$feed")" = "$before"
hook SessionEnd
test ! -e "$feed"
hook UserPromptSubmit
hook Denied
before=$(cat "$feed")
test "$(awk '{print $1}' "$feed")" = denied
hook Stop
hook PreToolUse '{"tool_name":"Bash"}'
hook PermissionRequest
test "$(cat "$feed")" = "$before"
hook UserPromptSubmit
test "$(awk '{print $1}' "$feed")" = turn
hook Denied
hook SessionEnd
test ! -e "$feed"
hook UserPromptSubmit
hook PreToolUse '{"tool_name":"request_user_input_async"}'
test "$(awk '{print $1}' "$feed")" = input
hook PostToolUse '{"tool_name":"request_user_input_async"}'
hook PreToolUse '{"tool_name":"Bash"}'
hook Stop
test "$(awk '{print $1}' "$feed")" = input
if rg -qw started "$feed"; then
  echo 'FAIL: finished question still has a running turn clock' >&2
  exit 1
fi
rg -q '^input last [0-9]+ ttl ' "$feed"
hook UserPromptSubmit
hook PreToolUse '{"tool_name":"request_user_input"}'
test "$(awk '{print $1}' "$feed")" = question
hook PostToolUse
test "$(awk '{print $1}' "$feed")" = turn
hook UserPromptSubmit
test "$(awk '{print $1}' "$feed")" = turn
hook SessionEnd
"$bin" agent hooks codex | jq -e '.hooks | length == 11' > /dev/null
echo 'PASS: native hook lifecycle, silent output, permission clock, concurrent counts, cleanup'
