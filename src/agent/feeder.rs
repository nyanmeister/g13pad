// SPDX-License-Identifier: GPL-3.0-or-later
//! The feeder: `g13map agent feed <event>`, the command a coding harness's hooks call. It
//! reads the hook's JSON on stdin, keys the feed off the terminal's X11 window id
//! (`$WINDOWID`), and updates that terminal's file. All the harness-specific knowledge lives
//! here — the tool-name-to-activity map, the permission-vs-idle message test — so the
//! watcher stays a feed reader. Codex's native lifecycle adapter lives beside it.
//!
//! It is called from hooks, so it must never fail the harness: every path exits 0, does its
//! best effort, and writes nothing to stdout (a hook's stdout can be injected back into the
//! session). Errors go to stderr at most.
use super::{Activity, Attention, State, Trailer};
use serde_json::Value;
use std::time::{Duration, SystemTime};

/// A turn's feed is kept alive this long without a fresh event: long enough for a slow tool
/// or a long think, short enough that a crashed harness's turn clears itself.
pub(super) const TURN_TTL: Duration = Duration::from_secs(900);
/// Ready/idle persist much longer: the finished turn should stay on the glass until the next
/// prompt, even if he steps away for a while.
pub(super) const REST_TTL: Duration = Duration::from_secs(6 * 3600);

/// What the event asks of the feed. Pure, so it is tested without env or files.
#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Set(State, Duration),
    Clear,
    Nothing,
}

/// The activity face for a tool name. Unmapped tools show their own upper-cased name rather
/// than a wrong guess; nothing renderable falls back to WORKING.
fn activity_for(tool: &str) -> &str {
    match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "Update" => "EDITING",
        "Bash" | "BashOutput" => "RUNNING BASH",
        "Read" => "READING",
        "Glob" | "Grep" => "SEARCHING",
        "WebSearch" => "WEB SEARCH",
        "WebFetch" => "FETCHING",
        "Task" | "Agent" => "SPAWN AGENT",
        "TodoWrite" => "PLANNING",
        "" => "WORKING",
        other => other,
    }
}

/// The subagent-spawning tools, which bump the background-agent count.
/// The ttl a phase is written with: a turn expires in minutes, a rest in hours.
fn ttl_for(state: &State) -> Duration {
    match state {
        State::Turn { .. } => TURN_TTL,
        _ => REST_TTL,
    }
}

/// The context window used, as the status line JSON reports it (`context_window.
/// used_percentage`, a float), rounded to whole percent and clamped to 0..=100.
fn context_pct(json: &Value) -> Option<super::Context> {
    let pct = json["context_window"]["used_percentage"].as_f64()?;
    if !pct.is_finite() {
        return None;
    }
    Some(super::Context::Rising(pct.round().clamp(0.0, 100.0) as u8))
}

/// Best-effort failure test on a PostToolUse `tool_response`. Tools differ; this catches the
/// common shapes (`is_error: true`, an `error` field) and errs toward no flash when unsure —
/// a missed flash is better than a false one. Refine against a real failed-tool payload.
fn tool_failed(json: &Value) -> bool {
    let r = &json["tool_response"];
    r["is_error"].as_bool() == Some(true)
        || r["isError"].as_bool() == Some(true)
        || (r["error"].is_string() && !r["error"].as_str().unwrap_or("").is_empty())
        || json["is_error"].as_bool() == Some(true)
}

/// Whether a Notification message is a permission prompt (blocked) rather than the idle
/// "waiting for your input" nudge. The permission messages name the tool or say "permission";
/// anything else is treated as idle.
fn is_permission(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("permission") || m.contains("approve") || m.contains("waiting for your approval")
}

/// The feed's trailer carried from the current state, if any.
fn carried(current: &Option<State>) -> Trailer {
    current.as_ref().map(State::trailer).unwrap_or_default()
}

/// The pure decision: given the event, its JSON, the current feed state and the clock, what
/// to write. `now` is the turn clock's zero for a fresh turn.
pub(crate) fn next(event: &str, json: &Value, current: Option<State>, now: SystemTime) -> Action {
    // A rejection is a terminal result. Later cleanup/Stop hooks must not paint
    // it green; only a new user request or session end clears the latch.
    if matches!(&current, Some(State::Denied { .. })) && !matches!(event, "prompt" | "end") {
        return Action::Nothing;
    }
    match event {
        // A new turn: reset the agent count, carry the previous turn's length as `last`.
        "prompt" => {
            let previous = carried(&current);
            let last = current.as_ref().and_then(|s| s.trailer().last);
            Action::Set(
                State::Turn {
                    activity: Activity::parse("THINKING"),
                    started: now,
                    trailer: Trailer {
                        last,
                        context: previous.context,
                        shells: previous.shells,
                        ..Default::default()
                    },
                },
                TURN_TTL,
            )
        }
        // A tool is about to run: set its face, keep the turn's start. A spawn is counted by
        // SubagentStart (`substart`), not here: a refused Agent call never starts one.
        "pretool" => {
            let tool = json["tool_name"].as_str().unwrap_or("");
            if tool == "AskUserQuestion" {
                return next("question", json, current, now);
            }
            let (started, trailer) = match current {
                Some(State::Turn {
                    started, trailer, ..
                }) => (started, trailer),
                Some(State::Ask {
                    started: Some(started),
                    trailer,
                    ..
                }) => (started, trailer),
                other => (now, carried(&other)),
            };
            Action::Set(
                State::Turn {
                    activity: Activity::parse(activity_for(tool)),
                    started,
                    trailer,
                },
                TURN_TTL,
            )
        }
        // A subagent started: one more background agent, the phase otherwise as it is (a
        // spawn can land while a permission is pending). No feed, no agent to count.
        "substart" => match current {
            Some(state) => {
                let mut trailer = state.trailer();
                trailer.agents = (trailer.agents + 1).min(super::MAX_AGENTS);
                Action::Set(state.with_trailer(trailer), ttl_for(&state))
            }
            None => Action::Nothing,
        },
        // The harness's status line, with its JSON on stdin: only the context ring changes,
        // and only on a live feed (a status line outliving its session must not revive one).
        "status" => match (current, context_pct(json)) {
            (Some(state), Some(pct)) if state.trailer().context != Some(pct) => {
                let mut trailer = state.trailer();
                trailer.context = Some(pct);
                Action::Set(state.with_trailer(trailer), ttl_for(&state))
            }
            _ => Action::Nothing,
        },
        // A tool finished: heartbeat the turn, flash orange on failure, face back to THINKING
        // until the next tool (so a long gap does not keep claiming a finished tool).
        "posttool" => {
            let (started, mut trailer) = match current {
                Some(State::Turn {
                    started, trailer, ..
                }) => (started, trailer),
                Some(State::Ask {
                    started: Some(started),
                    trailer,
                    ..
                }) => (started, trailer),
                other => (now, carried(&other)),
            };
            if tool_failed(json) {
                trailer.fail_at = Some(now);
            }
            Action::Set(
                State::Turn {
                    activity: Activity::parse("THINKING"),
                    started,
                    trailer,
                },
                TURN_TTL,
            )
        }
        // A harness explicitly reported a rejection. Freeze elapsed time and hold red.
        "deny" => {
            let (started, mut trailer) = match current {
                Some(State::Turn {
                    started, trailer, ..
                }) => (Some(started), trailer),
                Some(State::Ask {
                    started: Some(started),
                    trailer,
                    ..
                }) => (Some(started), trailer),
                other => (None, carried(&other)),
            };
            if let Some(started) = started {
                trailer.last = Some(now.duration_since(started).unwrap_or_default());
            }
            trailer.deny_at = Some(now);
            Action::Set(State::Denied { trailer }, REST_TTL)
        }
        // Attention: permission (blocked) or the idle nudge. Keep the trailer.
        "notify" | "ask" | "review" | "input" | "question" => {
            let message = json["message"].as_str().unwrap_or("");
            let trailer = carried(&current);
            let state = if event != "notify" || is_permission(message) {
                let started = match current {
                    Some(State::Turn { started, .. }) => Some(started),
                    Some(State::Ask {
                        started: Some(started),
                        ..
                    }) => Some(started),
                    _ if matches!(event, "input" | "question") => Some(now),
                    _ => None,
                };
                let attention = match event {
                    "review" => Attention::Review,
                    "input" => Attention::Question { asynchronous: true },
                    "question" => Attention::Question {
                        asynchronous: false,
                    },
                    _ => Attention::Permission,
                };
                State::Ask {
                    attention,
                    started,
                    trailer,
                }
            } else {
                State::Idle { trailer }
            };
            Action::Set(state, REST_TTL)
        }
        // The turn ended: freeze its length as `last`, show READY.
        "stop" => {
            let previous = carried(&current);
            let (last, agents) = match current {
                Some(State::Turn {
                    started, trailer, ..
                })
                | Some(State::Ask {
                    started: Some(started),
                    trailer,
                    ..
                }) => (
                    now.duration_since(started).unwrap_or_default(),
                    trailer.agents,
                ),
                Some(other) => {
                    let t = other.trailer();
                    (t.last.unwrap_or_default(), t.agents)
                }
                None => (Duration::ZERO, 0),
            };
            Action::Set(
                State::Ready {
                    trailer: Trailer {
                        agents,
                        last: Some(last),
                        ..previous
                    },
                },
                REST_TTL,
            )
        }
        // A subagent finished: one fewer background agent, turn otherwise unchanged.
        "substop" => {
            let (started, mut trailer) = match current {
                Some(State::Turn {
                    started, trailer, ..
                }) => (started, trailer),
                other => (now, carried(&other)),
            };
            trailer.agents = trailer.agents.saturating_sub(1);
            Action::Set(
                State::Turn {
                    activity: Activity::parse("THINKING"),
                    started,
                    trailer,
                },
                TURN_TTL,
            )
        }
        // The session ended: this terminal has no agent any more.
        "end" => Action::Clear,
        _ => Action::Nothing,
    }
}

/// The terminal's X11 window id from `$WINDOWID`, the key both sides share.
fn window() -> Option<u64> {
    std::env::var("WINDOWID").ok().and_then(|s| s.parse().ok())
}

/// A running Codex TUI: the window it draws in, the directory it was started in, and
/// when it started (for picking the newest of several in one directory).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tui {
    pub window: u64,
    pub cwd: std::path::PathBuf,
    pub started: u64,
}

/// Codex hooks run inside the app-server, whose `WINDOWID` is the terminal it was first
/// started from — for every later session, even after that terminal closes (found
/// 2026-10-20: every Codex feed landed under a dead window id and the pad never followed
/// the focused Codex terminal). The TUI in front of the user is a `codex` process whose
/// cwd is the session's `cwd` and whose own `WINDOWID` is the right key. Pick the newest
/// such TUI not already paired with another session; none means the env's window.
pub(crate) fn choose_tui(tuis: &[Tui], cwd: &std::path::Path, claimed: &[u64]) -> Option<u64> {
    let same_dir = || tuis.iter().filter(|t| t.cwd == cwd);
    let free = same_dir()
        .filter(|t| !claimed.contains(&t.window))
        .max_by_key(|t| t.started);
    free.or_else(|| same_dir().max_by_key(|t| t.started))
        .map(|t| t.window)
}

/// Every Codex TUI on this machine, from /proc: comm `codex`, not the app-server, with a
/// `WINDOWID`. Zombies and processes of other users read as nothing and are skipped.
fn tuis() -> Vec<Tui> {
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in procs.flatten() {
        let pid = entry.file_name();
        let Some(pid) = pid
            .to_str()
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let dir = std::path::Path::new("/proc").join(pid);
        let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        if comm.trim_end() != "codex" {
            continue;
        }
        let cmdline = std::fs::read(dir.join("cmdline")).unwrap_or_default();
        if cmdline.split(|b| *b == 0).any(|arg| arg == b"app-server") {
            continue;
        }
        let Ok(environ) = std::fs::read(dir.join("environ")) else {
            continue;
        };
        let Some(window) = environ
            .split(|b| *b == 0)
            .find_map(|kv| kv.strip_prefix(b"WINDOWID="))
            .and_then(|v| std::str::from_utf8(v).ok()?.parse().ok())
        else {
            continue;
        };
        let Ok(cwd) = std::fs::read_link(dir.join("cwd")) else {
            continue;
        };
        // Field 22 of /proc/PID/stat is the start time in clock ticks since boot.
        let started = std::fs::read_to_string(dir.join("stat"))
            .ok()
            .and_then(|st| st.rsplit(')').next().map(|tail| tail.to_string()))
            .and_then(|tail| tail.split_whitespace().nth(19).and_then(|v| v.parse().ok()))
            .unwrap_or(0);
        out.push(Tui {
            window,
            cwd,
            started,
        });
    }
    out
}

/// Where a session's pairing with its window is remembered (in the feeds dir, a name
/// `read_all` ignores). A session id is a UUID; anything else is not trusted as a path.
fn memo(session: &str) -> Option<std::path::PathBuf> {
    let clean = session.len() <= 64
        && !session.is_empty()
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    clean
        .then(|| super::feed::dir().ok())
        .flatten()
        .map(|d| d.join(format!("session-{session}")))
}

/// The window for a Codex event: the remembered one for its session, else the TUI whose
/// cwd matches (remembered from then on), else the hook's own `WINDOWID`.
fn codex_window(json: &Value, env_window: Option<u64>) -> Option<u64> {
    let session = json["session_id"].as_str().unwrap_or("");
    let path = memo(session);
    if let Some(win) = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(win);
    }
    let cwd = json["cwd"].as_str().map(std::path::PathBuf::from);
    let found = cwd.and_then(|cwd| {
        let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
        let claimed: Vec<u64> = super::feed::dir()
            .ok()
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("session-"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok()?.trim().parse().ok())
            .collect();
        choose_tui(&tuis(), &cwd, &claimed)
    });
    if let (Some(win), Some(p)) = (found, &path) {
        let tmp = p.with_extension("new");
        let _ = std::fs::write(&tmp, format!("{win}\n")).and_then(|_| std::fs::rename(&tmp, p));
    }
    found.or(env_window)
}

/// A session is over: forget its window pairing.
fn forget_codex(json: &Value) {
    if let Some(p) = memo(json["session_id"].as_str().unwrap_or("")) {
        let _ = std::fs::remove_file(p);
    }
}

/// `g13map agent ...`. Never fails the harness: a feed call always exits 0.
pub(crate) fn command(args: &[&str]) -> Result<String, String> {
    match args {
        ["feed", event] => {
            feed(event, false);
            Ok(String::new()) // silent: a hook's stdout can reach the session
        }
        ["codex", event] => {
            feed(event, true);
            Ok(String::new()) // silent: a hook's stdout can reach the session
        }
        ["focus"] => super::focus::command(false),
        ["focus", "next"] => super::focus::command(true),
        ["hooks"] => Ok(HOOKS.into()),
        ["hooks", "codex"] => Ok(super::codex::hooks(false)),
        ["hooks", "codex", "human"] => Ok(super::codex::hooks(true)),
        ["status"] => status(),
        _ => Err(USAGE.into()),
    }
}

/// Apply one event to this terminal's feed. Best-effort; a failure is logged, never fatal.
fn feed(event: &str, codex: bool) {
    let json: Value = {
        use std::io::Read;
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        serde_json::from_str(&buf).unwrap_or(Value::Null)
    };
    let win = if codex {
        codex_window(&json, window())
    } else {
        window()
    };
    let Some(win) = win else {
        // Not in a keyable terminal (no $WINDOWID, no TUI found): nothing to do, but do
        // not fail the hook.
        return;
    };
    // Tools and subagents can finish concurrently. Serialize the entire update,
    // including the read, so counts and atomic-write temporary files cannot race.
    use std::os::fd::AsRawFd;
    let lock = super::feed::dir().and_then(|d| {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(d.join(format!("{win}.lock")))
            .map_err(|e| e.to_string())
    });
    let Ok(lock) = lock else { return };
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return;
    }
    if codex
        && matches!(
            event,
            "PreToolUse" | "PostToolUse" | "UserPromptSubmit" | "Stop"
        )
    {
        super::shells::refresh(win, json["session_id"].as_str().unwrap_or(""));
    }
    let current = super::read_for(win);
    let action = if codex {
        super::codex::event(event, &json, current, SystemTime::now())
    } else {
        next(event, &json, current, SystemTime::now())
    };
    match action {
        Action::Set(mut state, ttl) => {
            if codex {
                if event == "PreCompact" {
                    let mut trailer = state.trailer();
                    trailer.context = None;
                    state = state.with_trailer(trailer);
                } else if let Some(path) = json["transcript_path"].as_str() {
                    let mut trailer = state.trailer();
                    trailer.context = super::context::read(std::path::Path::new(path))
                        .map(super::Context::Segmented);
                    state = state.with_trailer(trailer);
                }
            }
            if let Err(e) = super::write(win, Some(state), Some(ttl)) {
                eprintln!("g13map agent feed: {e}");
            }
        }
        Action::Clear => {
            super::shells::clear(win);
            if let Err(e) = super::write(win, None, None) {
                eprintln!("g13map agent feed: {e}");
            }
            if codex {
                forget_codex(&json);
            }
        }
        Action::Nothing => {}
    }
}

/// A human-readable list of the live agent feeds, for debugging the wiring.
fn status() -> Result<String, String> {
    let all = super::read_all();
    if all.is_empty() {
        return Ok("no live agent feeds".into());
    }
    let mut out = String::from("live agent feeds (window id: state):\n");
    for (win, _, state) in all {
        out.push_str(&format!("  {win}: {}\n", state.to_line()));
    }
    Ok(out)
}

const USAGE: &str =
    "usage: g13map agent feed EVENT   (EVENT: prompt pretool posttool deny notify stop
                                  substart substop status end)
       g13map agent codex EVENT   consume a native Codex hook event
       g13map agent focus [next]  raise the agent terminal needing you (next: cycle them)
       g13map agent hooks codex   print Codex hooks.json (review with /hooks)
       g13map agent hooks codex human   hooks for human-reviewed approvals
       g13map agent hooks        print the Claude Code hooks settings to wire it up
       g13map agent status       list the live per-terminal agent feeds";

const HOOKS: &str = r#"Add to ~/.claude/settings.json (merge with any existing "hooks"):

{
  "hooks": {
    "UserPromptSubmit": [{"hooks":[{"type":"command","command":"g13map agent feed prompt"}]}],
    "PreToolUse":  [{"matcher":"*","hooks":[{"type":"command","command":"g13map agent feed pretool"}]}],
    "PostToolUse": [{"matcher":"*","hooks":[{"type":"command","command":"g13map agent feed posttool"}]}],
    "Notification": [{"hooks":[{"type":"command","command":"g13map agent feed notify"}]}],
    "Stop":          [{"hooks":[{"type":"command","command":"g13map agent feed stop"}]}],
    "SubagentStart": [{"hooks":[{"type":"command","command":"g13map agent feed substart"}]}],
    "SubagentStop":  [{"hooks":[{"type":"command","command":"g13map agent feed substop"}]}],
    "SessionEnd":    [{"hooks":[{"type":"command","command":"g13map agent feed end"}]}]
  }
}

The terminal must export $WINDOWID (kitty, xterm and most X11 terminals do). Set the
profile for your terminal's window to the agent display: g13map profile agent NAME on,
and g13map focus set <class> NAME so it shows when that terminal is focused.

For the context ring, pipe the status line's JSON on: in your statusLine command,
after printing, run `g13map agent feed status` with the same stdin JSON (it reads
context_window.used_percentage and nothing else). Without it the ring is not drawn."#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn a_turn_runs_through_its_events() {
        // prompt starts a turn at `now`, carrying the previous turn's length.
        let prev = State::Ready {
            trailer: Trailer {
                last: Some(Duration::from_secs(120)),
                ..Default::default()
            },
        };
        let Action::Set(
            State::Turn {
                started,
                trailer,
                activity,
            },
            ttl,
        ) = next("prompt", &Value::Null, Some(prev), t(1_000))
        else {
            panic!("prompt should start a turn");
        };
        assert_eq!(activity.as_str(), "THINKING");
        assert_eq!(started, t(1_000));
        assert_eq!(trailer.last, Some(Duration::from_secs(120))); // carried
        assert_eq!(trailer.agents, 0); // reset
        assert_eq!(ttl, TURN_TTL);

        // pretool Bash sets the face and keeps the start.
        let turn = State::Turn {
            activity: Activity::parse("THINKING"),
            started: t(1_000),
            trailer: Trailer::default(),
        };
        let Action::Set(
            State::Turn {
                activity, started, ..
            },
            _,
        ) = next(
            "pretool",
            &json!({"tool_name": "Bash"}),
            Some(turn.clone()),
            t(2_000),
        )
        else {
            panic!("pretool should keep a turn");
        };
        assert_eq!(activity.as_str(), "RUNNING BASH");
        assert_eq!(started, t(1_000)); // start preserved, not reset to 2_000

        // An Agent call is not yet an agent (it can be refused): the count waits for
        // SubagentStart, which keeps the phase it lands on, face included.
        let Action::Set(State::Turn { trailer, .. }, _) = next(
            "pretool",
            &json!({"tool_name": "Agent"}),
            Some(turn.clone()),
            t(2_000),
        ) else {
            panic!();
        };
        assert_eq!(trailer.agents, 0);
        let Action::Set(
            State::Turn {
                trailer,
                activity,
                started,
            },
            ttl,
        ) = next(
            "substart",
            &json!({"agent_id": "a1"}),
            Some(turn.clone()),
            t(2_100),
        )
        else {
            panic!("substart should keep the turn");
        };
        assert_eq!(
            (trailer.agents, activity.as_str(), started),
            (1, "THINKING", t(1_000))
        );
        assert_eq!(ttl, TURN_TTL);
        let pending = State::Ask {
            attention: Attention::Permission,
            started: Some(t(1_000)),
            trailer: Trailer {
                agents: 3,
                ..Default::default()
            },
        };
        let Action::Set(State::Ask { trailer, .. }, ttl) =
            next("substart", &Value::Null, Some(pending), t(2_200))
        else {
            panic!("substart should keep a pending ask");
        };
        assert_eq!((trailer.agents, ttl), (4, REST_TTL));
        assert!(matches!(
            next("substart", &Value::Null, None, t(0)),
            Action::Nothing
        ));

        // The status line sets the context ring on whatever phase is live, once per
        // change, and never conjures a feed where there is none.
        let status = json!({"context_window": {"used_percentage": 41.6}});
        let Action::Set(State::Turn { trailer, .. }, ttl) =
            next("status", &status, Some(turn.clone()), t(2_300))
        else {
            panic!("status should keep the turn");
        };
        assert_eq!(
            (trailer.context, ttl),
            (Some(super::super::Context::Rising(42)), TURN_TTL)
        );
        let same = turn.with_trailer(Trailer {
            context: Some(super::super::Context::Rising(42)),
            ..Default::default()
        });
        assert!(matches!(
            next("status", &status, Some(same), t(2_400)),
            Action::Nothing
        ));
        assert!(matches!(
            next("status", &status, None, t(0)),
            Action::Nothing
        ));
        assert!(matches!(
            next(
                "status",
                &json!({"context_window": {}}),
                Some(turn.clone()),
                t(0)
            ),
            Action::Nothing
        ));
        let over = json!({"context_window": {"used_percentage": 140.0}});
        let Action::Set(State::Turn { trailer, .. }, _) =
            next("status", &over, Some(turn.clone()), t(0))
        else {
            panic!();
        };
        assert_eq!(trailer.context, Some(super::super::Context::Rising(100)));

        // posttool with a failure flashes; substop drops an agent.
        let busy = State::Turn {
            activity: Activity::parse("RUNNING BASH"),
            started: t(1_000),
            trailer: Trailer {
                agents: 2,
                ..Default::default()
            },
        };
        let Action::Set(State::Turn { trailer, .. }, _) = next(
            "posttool",
            &json!({"tool_response": {"is_error": true}}),
            Some(busy.clone()),
            t(3_000),
        ) else {
            panic!();
        };
        assert_eq!(trailer.fail_at, Some(t(3_000)));
        let Action::Set(State::Turn { trailer, .. }, _) =
            next("substop", &Value::Null, Some(busy), t(3_500))
        else {
            panic!();
        };
        assert_eq!(trailer.agents, 1); // 2 -> 1

        // stop freezes the elapsed length as `last`.
        let running = State::Turn {
            activity: Activity::parse("EDITING"),
            started: t(1_000),
            trailer: Trailer {
                agents: 1,
                ..Default::default()
            },
        };
        let Action::Set(State::Ready { trailer }, ttl) =
            next("stop", &Value::Null, Some(running), t(4_500))
        else {
            panic!("stop should go ready");
        };
        assert_eq!(trailer.last, Some(Duration::from_millis(3_500))); // 4500 - 1000
        assert_eq!(trailer.agents, 1);
        assert_eq!(ttl, REST_TTL);
    }

    #[test]
    fn notifications_split_permission_from_idle() {
        let perm = next(
            "notify",
            &json!({"message": "Claude needs your permission to use Bash"}),
            None,
            t(0),
        );
        assert!(matches!(perm, Action::Set(State::Ask { .. }, _)));
        let idle = next(
            "notify",
            &json!({"message": "Claude is waiting for your input"}),
            None,
            t(0),
        );
        assert!(matches!(idle, Action::Set(State::Idle { .. }, _)));
    }

    #[test]
    fn claude_question_preserves_turn_clock_until_the_answer_returns() {
        let turn = State::Turn {
            activity: Activity::parse("THINKING"),
            started: t(1000),
            trailer: Trailer::default(),
        };
        let Action::Set(question, _) = next(
            "pretool",
            &json!({"tool_name":"AskUserQuestion"}),
            Some(turn),
            t(2000),
        ) else {
            panic!()
        };
        assert_eq!(question.to_line(), "question started 1000");
        let Action::Set(resumed, _) = next("posttool", &Value::Null, Some(question), t(3000))
        else {
            panic!()
        };
        assert_eq!(resumed.to_line(), "turn THINKING started 1000");
    }

    #[test]
    fn denial_is_latched_until_a_new_prompt_or_session_end() {
        let turn = State::Turn {
            activity: Activity::parse("EDITING"),
            started: t(1_000),
            trailer: Trailer::default(),
        };
        let Action::Set(denied, ttl) = next("deny", &Value::Null, Some(turn), t(2_000)) else {
            panic!();
        };
        assert_eq!(denied.to_line(), "denied last 1000 deny 2000");
        assert_eq!(ttl, REST_TTL);
        for event in ["stop", "pretool", "posttool", "notify", "deny"] {
            assert_eq!(
                next(event, &Value::Null, Some(denied.clone()), t(3_000)),
                Action::Nothing
            );
        }
        assert!(matches!(
            next("prompt", &Value::Null, Some(denied.clone()), t(4_000)),
            Action::Set(State::Turn { .. }, _)
        ));
        assert_eq!(
            next("end", &Value::Null, Some(denied), t(5_000)),
            Action::Clear
        );
        assert_eq!(next("end", &Value::Null, None, t(0)), Action::Clear);
        assert_eq!(next("bogus", &Value::Null, None, t(0)), Action::Nothing);
    }

    #[test]
    fn a_codex_session_is_keyed_to_the_tui_in_its_directory() {
        use std::path::PathBuf;
        let tui = |window, cwd: &str, started| Tui {
            window,
            cwd: PathBuf::from(cwd),
            started,
        };
        let tuis = [
            tui(100, "/home/u/a", 10), // older session in a
            tui(200, "/home/u/a", 20), // newer session in a
            tui(300, "/home/u/b", 30),
        ];
        let a = PathBuf::from("/home/u/a");
        // Hand-derived: nothing claimed -> the newest TUI in that directory (200).
        assert_eq!(choose_tui(&tuis, &a, &[]), Some(200));
        // 200 already belongs to another session -> the free one (100).
        assert_eq!(choose_tui(&tuis, &a, &[200]), Some(100));
        // Both claimed (a stale memo, say) -> still the newest rather than nothing.
        assert_eq!(choose_tui(&tuis, &a, &[100, 200]), Some(200));
        // A directory with no TUI -> None, so the caller falls back to the env window.
        assert_eq!(choose_tui(&tuis, &PathBuf::from("/nowhere"), &[]), None);
        // Session ids that are not UUID-shaped never become a path.
        assert!(memo("../../etc/passwd").is_none());
        assert!(memo("").is_none());
    }

    #[test]
    fn unmapped_tools_show_their_own_name() {
        assert_eq!(activity_for("Edit"), "EDITING");
        assert_eq!(activity_for("SomeNewTool"), "SomeNewTool");
        assert_eq!(activity_for(""), "WORKING");
    }
}
