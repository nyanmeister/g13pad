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
fn spawns_agent(tool: &str) -> bool {
    matches!(tool, "Task" | "Agent")
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
            let last = current.as_ref().and_then(|s| s.trailer().last);
            Action::Set(
                State::Turn {
                    activity: Activity::parse("THINKING"),
                    started: now,
                    trailer: Trailer {
                        last,
                        ..Default::default()
                    },
                },
                TURN_TTL,
            )
        }
        // A tool is about to run: set its face, keep the turn's start, bump agents for a spawn.
        "pretool" => {
            let tool = json["tool_name"].as_str().unwrap_or("");
            if tool == "AskUserQuestion" {
                return next("question", json, current, now);
            }
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
            if spawns_agent(tool) {
                trailer.agents = (trailer.agents + 1).min(super::MAX_AGENTS);
            }
            Action::Set(
                State::Turn {
                    activity: Activity::parse(activity_for(tool)),
                    started,
                    trailer,
                },
                TURN_TTL,
            )
        }
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
                        ..Default::default()
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
        ["hooks"] => Ok(HOOKS.into()),
        ["hooks", "codex"] => Ok(super::codex::hooks(false)),
        ["hooks", "codex", "human"] => Ok(super::codex::hooks(true)),
        ["status"] => status(),
        _ => Err(USAGE.into()),
    }
}

/// Apply one event to this terminal's feed. Best-effort; a failure is logged, never fatal.
fn feed(event: &str, codex: bool) {
    let Some(win) = window() else {
        // Not in a keyable terminal (no $WINDOWID): nothing to do, but do not fail the hook.
        return;
    };
    let json: Value = {
        use std::io::Read;
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        serde_json::from_str(&buf).unwrap_or(Value::Null)
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
    let current = super::read_for(win);
    let action = if codex {
        super::codex::event(event, &json, current, SystemTime::now())
    } else {
        next(event, &json, current, SystemTime::now())
    };
    match action {
        Action::Set(state, ttl) => {
            if let Err(e) = super::write(win, Some(state), Some(ttl)) {
                eprintln!("g13map agent feed: {e}");
            }
        }
        Action::Clear => {
            if let Err(e) = super::write(win, None, None) {
                eprintln!("g13map agent feed: {e}");
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
    "usage: g13map agent feed EVENT   (EVENT: prompt pretool posttool deny notify stop substop end)
       g13map agent codex EVENT   consume a native Codex hook event
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
    "SubagentStop":  [{"hooks":[{"type":"command","command":"g13map agent feed substop"}]}],
    "SessionEnd":    [{"hooks":[{"type":"command","command":"g13map agent feed end"}]}]
  }
}

The terminal must export $WINDOWID (kitty, xterm and most X11 terminals do). Set the
profile for your terminal's window to the agent display: g13map profile agent NAME on,
and g13map focus set <class> NAME so it shows when that terminal is focused."#;

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

        // A Task spawns an agent: the count goes up.
        let Action::Set(State::Turn { trailer, .. }, _) = next(
            "pretool",
            &json!({"tool_name": "Task"}),
            Some(turn.clone()),
            t(2_000),
        ) else {
            panic!();
        };
        assert_eq!(trailer.agents, 1);

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
    fn unmapped_tools_show_their_own_name() {
        assert_eq!(activity_for("Edit"), "EDITING");
        assert_eq!(activity_for("SomeNewTool"), "SomeNewTool");
        assert_eq!(activity_for(""), "WORKING");
        assert!(spawns_agent("Task") && !spawns_agent("Bash"));
    }
}
