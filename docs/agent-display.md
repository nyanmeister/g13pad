# Coding assistant display

The **Agent** display shows the current activity, a turn stopwatch, background-agent
count and the previous turn's duration. Permissions blink yellow. Questions blink
slowly in the working colour, then green when the turn finishes with a question
pending. Reported tool errors flash orange.
The pulse is a display animation, not a signal from the model.
The stopwatch and completed-turn duration use **HH:MM:SS.CC**.
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
their states need to remain separate. A shared Codex app-server inherits its launch
environment, so check its `WINDOWID` when several terminals share that server.

## Claude Code

`g13map agent hooks` prints the entries to merge into `~/.claude/settings.json`.
Keep any existing hooks. The hook command is `g13map agent feed EVENT`.

## Codex

Uses native hooks available in Codex app-server 0.163.0. These supply prompt, tool,
permission, compaction, subagent, stop, interruption and session-end events. Read the
[Codex hook reference](https://learn.chatgpt.com/docs/hooks) for supported harnesses
and trust review. Hosted tools can be invisible to tool hooks; THINKING means an
active turn with no currently reported tool, not proof of model activity.

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
feed. Agent counts use SubagentStart/Stop, so a rejected spawn does not add one.
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
