# Coding assistant display

The **Agent** display shows the current activity, a turn stopwatch, the previous turn's
duration and, in the lower right, the background-agent count. Permissions blink yellow.
Questions blink slowly in the working colour, then green when the turn finishes with a
question pending. Reported tool errors flash orange.
The pulse (a resting lub-dub) is a display animation, not a signal from the model.
The stopwatch and completed-turn duration use **HH:MM:SS.CC**. TURN and LAST share
numeric columns, with hours, minutes and seconds aligned; LAST omits hundredths
while another turn is running.

**Context ring.** When the harness reports how full the model's context window is, a
ring draws clockwise around the pulse by that fraction, and a lit disc rises inside it by
the same fraction with the heart dark where the disc covers it: at a third the heart's
feet are under water, near full it is a dark heart in a bright disc. Claude Code reports
it through the status line (below). Codex uses a compact GX-style perimeter with twelve
progress segments, keeping the pulsing heart clear. An unknown reading draws no gauge.
The latest reading survives normal turn/phase changes; compaction clears it until a new
measurement is available.

**Background shells.** Codex's SHELL/SHELLS count sits above AGENT/AGENTS. Counts are
right-aligned in a two-digit field and the words share one column. The observer uses
Codex's thread/tool-call process tags, grouping a sandbox, shell and child programs as
one session. The display checks recorded PIDs and their start times, so an exited command
disappears without another hook or tool poll. Discovery refreshes at the next tool start,
tool completion, prompt or turn end; a newly backgrounded command can appear at the next
such event. Untracked/detached jobs and processes whose
environment cannot be read may be absent. Claude's background-shell collection is not
implemented; its subagent counter still works.

**A key that raises the agent's terminal.** `g13map agent focus` activates the terminal
window of the agent waiting on you (a permission or question, the longest-pending first),
else the one the pad shows; `g13map agent focus next` cycles through the live agent
terminals in window-id order from the focused one. Both are meant for a run binding
(`g13map profile run NAME G22 g13map agent focus`, and another key for `next`), work on
i3 and any EWMH window manager, and say what they raised. Wayland sessions have no
global window ids to raise by, like the window rules.
Long activity labels scroll at 20 pixels per second, holding for one second at
each end. A new activity starts at the left; repeated feed updates keep scrolling.

Select **Agent** under a terminal profile's **Shows**, or run:

```sh
g13map profile agent coding on
g13map focus set kitty coding
```

Use an existing profile name and your terminal's window class. Hooks require the
terminal's exported `WINDOWID` and the same `XDG_RUNTIME_DIR` as `g13map-watch`.
The display follows the last focused agent terminal when you focus something else.
Several sessions in one terminal window share its feed; use separate windows when
their states need to remain separate. Codex hooks run inside a shared app-server whose
`WINDOWID` is the terminal it was first started from, so the feeder keys a Codex session
to the `codex` process whose working directory matches the hook's `cwd` and uses that
process's `WINDOWID`; the pairing is remembered per session (a `session-ID` file beside
the feeds) until SessionEnd. Two Codex sessions started in the same directory get the
newest unpaired terminal; if no TUI matches, the hook's own `WINDOWID` is used.
The pulse beats sixty times a minute as a lub-dub; it is a display animation.

## Claude Code

`g13map agent hooks` prints the entries to merge into `~/.claude/settings.json`.
Keep any existing hooks. The hook command is `g13map agent feed EVENT`. The agent count
follows `SubagentStart`/`SubagentStop` (since 0.3.9; before, an `Agent` tool call counted
at once, so a refused spawn stayed counted until the turn ended).

For the context ring, the status line command hands its stdin JSON on to
`g13map agent feed status` after printing (it reads `context_window.used_percentage` and
nothing else, updates a live feed only, and never creates one). A Python status line:

```python
subprocess.run(["g13map", "agent", "feed", "status"], input=json.dumps(data),
               text=True, timeout=1, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
```

## Codex

Uses native hooks available in Codex app-server 0.163.0. These supply prompt, tool,
permission, compaction, subagent, stop, interruption and session-end events. Read the
[Codex hook reference](https://learn.chatgpt.com/docs/hooks) for supported harnesses
and trust review. Hosted tools can be invisible to tool hooks; THINKING means an
active turn with no currently reported tool, not proof of model activity.

For context, the adapter reads the latest usage metadata from the hook's `transcript_path`:
`info.last_token_usage.total_tokens / info.model_context_window`, rounded to a whole percent.
It never uses cumulative usage, cached-token subtraction, or rate-limit budgets. The reader
starts with an 8 KiB tail and grows to at most 1 MiB, returning unknown if no supported
measurement is found. The rollout format is not a stable hook interface; unsupported
formats show an empty gauge. These samples can lag the current model request.

`g13map agent hooks codex` prints valid `hooks.json`. Merge its `hooks` entries into
`~/.codex/hooks.json`, preserving other hooks. Each command calls
`g13map agent codex EVENT`; use an absolute executable path if `g13map` is not on
the app-server's PATH. Open `/hooks` in Codex to review and trust the new entries.
Reload/resume the session if its hook configuration was already loaded.

With automatic approval review, Codex's PermissionRequest shows **REMOTE CHECK**
in steady yellow. Human approval shows **PERMISSIONS** in blinking yellow. The native
hook payload does not identify who reviews the request: for a Codex setup using
human approval instead, generate `g13map agent hooks codex human` and review those
definitions. This changes only PermissionRequest's command to the explicit
HumanPermissionRequest adapter event. Claude Code's human notifications retain
their blinking state.

Codex's request_user_input tools show **QUESTION**, with one-second normal and
inverted holds (one full blink every two seconds). It keeps the working backlight colour
while the turn runs, then green after Stop, with the elapsed turn frozen. An
asynchronous question stays visible through independent work and Stop until the
next user prompt or session end; a synchronous question clears when its tool
returns. Claude Code's AskUserQuestion uses the same synchronous presentation.
Actual permissions retain their faster yellow blink; remote checks stay steady.

Hooks run synchronously with a three-second timeout. The feeder is silent, exits
successfully even without a terminal ID, stores no prompt/tool/transcript contents,
and makes no approval decisions. It only writes the existing per-terminal display
feed and numeric PID/start-time sidecar. Agent counts use SubagentStart/Stop, so a rejected spawn does not add one.
Approval waits preserve the turn clock across hook invocations. Interruptions show
the completed duration. SessionEnd removes the feed; crashes expire after 15
minutes during a turn, or six hours after completion.

Error flashes depend on error metadata in the hook response. Codex app-server
0.163.0's Bash hook sent an empty string, without an exit status, for an empty-output
`exit 17` command. The feeder cannot distinguish that from empty success output;
such shell failures do not produce an orange flash.

## Denied requests

A reported denial shows **DENIED** in red, blinks three times, then holds steady red.
It stays through later tools, permission checks and Stop so cleanup cannot paint
it green. The next user prompt clears it; SessionEnd removes it. Without either,
it expires after six hours like a finished turn.

The explicit adapter inputs are `g13map agent codex Denied` and
`g13map agent feed deny`. A policy or harness integration that receives a genuine
rejection can call these with JSON on stdin (or `null`). They report the decision
to the screen; they do not make or override approval decisions.

Codex app-server 0.163.0's documented lifecycle events have PermissionRequest but
no separate rejection-result hook. The default hooks therefore cannot promise
automatic detection of classifier denials or refusals before the session runs.
`Denied` is an explicit adapter event, not a native hook to add to hooks.json.
Ordinary tool failures remain orange; missing output or inactivity is not denial.

## Diagnose the boundary

`g13map agent status` lists live feeds. Files are named by decimal window ID under
`$XDG_RUNTIME_DIR/g13map-g13-0.agents/`; `.lock` files serialize hook updates and are
not feeds. If the list is empty, inspect hook trust, the command path, `WINDOWID`
and `XDG_RUNTIME_DIR` before changing the renderer. `agent hooks codex` alone does
not install or trust hooks.

After building, `bash tools/check-agent-hooks.sh target/release/g13map` exercises
the real command in a temporary runtime, including concurrent subagent events.
It does not open the device or change the desktop. Unit tests cover native event
mapping and randomized valid sequences through the feed parser. A synthetic hook
check establishes the adapter contract; confirm a real harness turn separately.
