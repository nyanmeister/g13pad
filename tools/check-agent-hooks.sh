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
# The Claude Code path: a spawn counts at SubagentStart, not at the Agent call; the
# status line's JSON sets the context word once per change and never creates a feed.
cfeed() {
  local out="$scratch/cstdout"
  printf '%s\n' "${2:-null}" | "$bin" agent feed "$1" > "$out"
  test ! -s "$out"
}
cfeed_file="$XDG_RUNTIME_DIR/g13map-pipe.agents/424243"
WINDOWID=424243 cfeed prompt
WINDOWID=424243 cfeed pretool '{"tool_name":"Agent"}'
if rg -q 'agents' "$cfeed_file"; then echo 'FAIL: an Agent call counted before it started' >&2; exit 1; fi
WINDOWID=424243 cfeed substart '{"agent_id":"a1","agent_type":"Explore"}'
WINDOWID=424243 cfeed substart '{"agent_id":"a2","agent_type":"Explore"}'
rg -q ' agents 2 ' "$cfeed_file"
WINDOWID=424243 cfeed status '{"context_window":{"used_percentage":41.6}}'
rg -q ' agents 2 context 42 ' "$cfeed_file" || { echo "FAIL: context missing: $(cat "$cfeed_file")" >&2; exit 1; }
WINDOWID=424243 cfeed substop
WINDOWID=424243 cfeed substop
rg -q ' context 42 ' "$cfeed_file"
if rg -q 'agents' "$cfeed_file"; then echo 'FAIL: stopped subagents still counted' >&2; exit 1; fi
WINDOWID=424244 cfeed status '{"context_window":{"used_percentage":50}}'
test ! -e "$XDG_RUNTIME_DIR/g13map-pipe.agents/424244"
WINDOWID=424243 cfeed end
test ! -e "$cfeed_file"
# With no live feed (an empty private runtime), `agent focus` says so and touches no display.
mkdir -p "$scratch/empty-runtime"
if XDG_RUNTIME_DIR="$scratch/empty-runtime" env -u DISPLAY -u I3SOCK -u SWAYSOCK -u WAYLAND_DISPLAY "$bin" agent focus > "$scratch/focus-out" 2>&1; then
  echo 'FAIL: agent focus succeeded with nothing to focus' >&2; exit 1
fi
rg -q 'no live agent feed' "$scratch/focus-out"
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
# A Codex session is keyed to the TUI in front of the user, not to the app-server's
# inherited WINDOWID: a process named codex with its own WINDOWID and the session's cwd.
mkdir -p "$scratch/bin" "$scratch/work"
cp "$(command -v sleep)" "$scratch/bin/codex"
(cd "$scratch/work" && WINDOWID=777 exec "$scratch/bin/codex" 300) &
tui_pid=$!
trap 'kill "$tui_pid" 2>/dev/null || true; rm -rf -- "$scratch"' EXIT
sleep 0.2
session=$(printf '{"cwd":"%s","session_id":"0199-test-session-1"}' "$scratch/work")
printf '%s\n' "$session" | "$bin" agent codex UserPromptSubmit > "$scratch/tui-out"
test ! -s "$scratch/tui-out"
tui_feed="$XDG_RUNTIME_DIR/g13map-pipe.agents/777"
test "$(awk '{print $1}' "$tui_feed")" = turn
test ! -e "$feed"
test "$(cat "$XDG_RUNTIME_DIR/g13map-pipe.agents/session-0199-test-session-1")" = 777
# The pairing holds even when the cwd no longer matches (the memo wins).
printf '{"cwd":"/","session_id":"0199-test-session-1"}\n' | "$bin" agent codex Stop > /dev/null
test "$(awk '{print $1}' "$tui_feed")" = ready
# A session whose directory has no TUI falls back to the hook's own WINDOWID.
printf '{"cwd":"/","session_id":"0199-test-session-2"}\n' | "$bin" agent codex UserPromptSubmit > /dev/null
test "$(awk '{print $1}' "$feed")" = turn
printf '{"cwd":"/","session_id":"0199-test-session-2"}\n' | "$bin" agent codex SessionEnd > /dev/null
printf '%s\n' "$session" | "$bin" agent codex SessionEnd > /dev/null
test ! -e "$tui_feed"
test ! -e "$XDG_RUNTIME_DIR/g13map-pipe.agents/session-0199-test-session-1"
kill "$tui_pid"
wait "$tui_pid" 2>/dev/null || true

# Real-form usage metadata, with cumulative totals deliberately disagreeing.
jq -nc '{type:"event_msg",payload:{type:"token_count",info:{last_token_usage:{total_tokens:125},total_token_usage:{total_tokens:999999},model_context_window:1000}}}' > "$scratch/usage.jsonl"
usage_payload=$(jq -nc --arg path "$scratch/usage.jsonl" '{transcript_path:$path,tool_name:"Bash"}')
hook UserPromptSubmit "$usage_payload"
rg -q ' context_gx 13 ' "$feed"
hook PreCompact "$usage_payload"
if rg -q context "$feed"; then echo 'FAIL: compaction kept stale context' >&2; exit 1; fi
printf '%s\n' '{"type":"compacted"}' >> "$scratch/usage.jsonl"
hook PostCompact "$usage_payload"
if rg -q context "$feed"; then echo 'FAIL: old usage returned after compaction' >&2; exit 1; fi
hook SessionEnd

# A real background process leaves the count on exit without another feed update.
probe_thread="g13map-shell-test-$BASHPID"
shell_payload=$(jq -nc --arg session "$probe_thread" '{session_id:$session,tool_name:"Bash"}')
hook UserPromptSubmit "$shell_payload"
env CODEX_THREAD_ID="$probe_thread" CODEX_TOOL_CALL_ID=background-probe sleep 1 &
shell_pid=$!
trap 'kill "$shell_pid" 2>/dev/null || true; rm -rf -- "$scratch"' EXIT
hook PreToolUse "$shell_payload"
if ! "$bin" agent status | rg -q '424242: .*shells 1'; then
  echo 'FAIL: background shell absent at next tool start' >&2; exit 1
fi
wait "$shell_pid"
if "$bin" agent status | rg -q '424242: .*shells'; then echo 'FAIL: exited shell still counted' >&2; exit 1; fi
hook SessionEnd "$shell_payload"
test ! -e "$XDG_RUNTIME_DIR/g13map-pipe.agents/shells-424242"
"$bin" agent hooks codex | jq -e '.hooks | length == 11' > /dev/null
echo 'PASS: hook lifecycle, silent output, clocks, concurrent counts, TUI keying, context metadata, background shell exit, cleanup'
