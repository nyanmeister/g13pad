// SPDX-License-Identifier: GPL-3.0-or-later
//! Codex lifecycle events, adapted to the harness-independent agent feed.
use super::{
    feeder::{next, Action},
    Attention, State,
};
use serde_json::Value;
use std::time::SystemTime;

/// Valid user-level hooks.json. Synchronous, tiny updates retain lifecycle ordering;
/// no prompt, tool arguments, or transcript contents are stored by the feeder.
pub(super) fn hooks(human_approvals: bool) -> String {
    let events = [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "PreCompact",
        "PostCompact",
        "SubagentStart",
        "SubagentStop",
        "Stop",
        "Interrupt",
        "SessionEnd",
    ];
    let hooks: serde_json::Map<String, Value> = events
        .into_iter()
        .map(|event| {
            let command_event = if human_approvals && event == "PermissionRequest" { "HumanPermissionRequest" } else { event };
            (
                event.to_string(),
                serde_json::json!([{"hooks":[{
                    "type":"command", "command":format!("g13map agent codex {command_event}"), "timeout":3
                }]}]),
            )
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({"hooks":hooks})).unwrap()
}

pub(super) fn event(event: &str, json: &Value, current: Option<State>, now: SystemTime) -> Action {
    // Explicit adapter input, not an invented Codex lifecycle hook. A caller
    // that actually received a rejection can report it without text guessing.
    if event == "Denied" {
        return next("deny", json, current, now);
    }
    // Native lifecycle events count successful starts, rather than attempts to call
    // spawn_agent. Do not also increment the Claude tool-based counter.
    if matches!(event, "SubagentStart" | "SubagentStop") {
        let mut state = current.unwrap_or(State::Turn {
            activity: super::Activity::parse("THINKING"),
            started: now,
            trailer: super::Trailer::default(),
        });
        let trailer = match &mut state {
            State::Turn { trailer, .. }
            | State::Ask { trailer, .. }
            | State::Ready { trailer }
            | State::Denied { trailer }
            | State::Idle { trailer }
            | State::Waiting { trailer } => trailer,
        };
        trailer.agents = if event == "SubagentStart" {
            trailer.agents.saturating_add(1).min(super::MAX_AGENTS)
        } else {
            trailer.agents.saturating_sub(1)
        };
        let ttl = if matches!(state, State::Turn { .. }) {
            super::feeder::TURN_TTL
        } else {
            super::feeder::REST_TTL
        };
        return Action::Set(state, ttl);
    }
    // An async question returns before the user answers. Keep its attention
    // state through independent work; the next user prompt acknowledges it.
    if matches!(
        &current,
        Some(State::Ask {
            attention: Attention::Question { asynchronous: true },
            ..
        })
    ) && !matches!(event, "UserPromptSubmit" | "SessionEnd")
    {
        if matches!(event, "Stop" | "Interrupt") {
            if let Some(State::Ask {
                attention,
                started: Some(started),
                mut trailer,
            }) = current
            {
                trailer.last = Some(now.duration_since(started).unwrap_or_default());
                return Action::Set(
                    State::Ask {
                        attention,
                        started: None,
                        trailer,
                    },
                    super::feeder::REST_TTL,
                );
            }
        }
        return Action::Nothing;
    }
    let mapped = match event {
        "UserPromptSubmit" => "prompt",
        "PreToolUse" | "PreCompact" => {
            match json["tool_name"].as_str() {
                Some("request_user_input" | "functions.request_user_input") => {
                    return next("question", json, current, now)
                }
                Some("request_user_input_async" | "functions.request_user_input_async") => {
                    return next("input", json, current, now)
                }
                _ => {}
            }
            let tool = if event == "PreCompact" {
                "COMPACTING"
            } else {
                match json["tool_name"].as_str().unwrap_or("") {
                    "apply_patch" => "Edit",
                    "exec_command" | "shell" | "shell_command" => "Bash",
                    "spawn_agent" | "Agent" => "SPAWN AGENT",
                    "update_plan" => "TodoWrite",
                    "request_user_input" | "request_user_input_async" => "ASKING USER",
                    other => other,
                }
            };
            return next(
                "pretool",
                &serde_json::json!({"tool_name":tool}),
                current,
                now,
            );
        }
        "PostToolUse" | "PostCompact" => "posttool",
        "PermissionRequest" => "review",
        "HumanPermissionRequest" => "ask",
        "Stop" | "Interrupt" => "stop",
        "SessionEnd" => "end",
        // SessionStart can be emitted on resume/compaction during a turn. Leave
        // its clock alone; UserPromptSubmit or the first tool supplies the state.
        _ => return Action::Nothing,
    };
    next(mapped, json, current, now)
}

#[cfg(test)]
mod tests {
    use super::super::{Activity, Trailer};
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    fn at(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    fn state(action: Action) -> State {
        match action {
            Action::Set(state, _) => state,
            other => panic!("expected state, got {other:?}"),
        }
    }

    #[test]
    fn native_codex_lifecycle_keeps_the_turn_clock_and_counts_actual_starts() {
        let mut s = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        s = state(event(
            "PreToolUse",
            &json!({"tool_name":"spawn_agent"}),
            Some(s),
            at(1200),
        ));
        assert_eq!(s.trailer().agents, 0); // An attempted spawn is not a started agent.
        s = state(event("SubagentStart", &Value::Null, Some(s), at(1300)));
        assert_eq!(s.trailer().agents, 1);
        s = state(event(
            "PreToolUse",
            &json!({"tool_name":"apply_patch"}),
            Some(s),
            at(2000),
        ));
        let State::Turn {
            activity, started, ..
        } = &s
        else {
            panic!()
        };
        assert_eq!(activity.as_str(), "EDITING");
        assert_eq!(*started, at(1000));
        s = state(event(
            "PostToolUse",
            &json!({"tool_response":{"isError":true}}),
            Some(s),
            at(2500),
        ));
        assert_eq!(s.trailer().fail_at, Some(at(2500)));
        s = state(event("SubagentStop", &Value::Null, Some(s), at(3000)));
        assert_eq!(s.trailer().agents, 0);
        s = state(event("Stop", &Value::Null, Some(s), at(4500)));
        assert!(matches!(s, State::Ready { .. }));
        assert_eq!(s.trailer().last, Some(Duration::from_millis(3500)));
        assert_eq!(
            event("SessionEnd", &Value::Null, Some(s), at(5000)),
            Action::Clear
        );
        assert_eq!(
            event("FutureEvent", &Value::Null, None, at(5000)),
            Action::Nothing
        );
    }

    #[test]
    fn permission_interrupt_and_compaction_are_explicit_events() {
        let turn = State::Turn {
            activity: Activity::parse("THINKING"),
            started: at(1000),
            trailer: Trailer::default(),
        };
        assert!(matches!(
            event(
                "PermissionRequest",
                &Value::Null,
                Some(turn.clone()),
                at(2000)
            ),
            Action::Set(State::Ask { .. }, _)
        ));
        let compact = state(event(
            "PreCompact",
            &Value::Null,
            Some(turn.clone()),
            at(2000),
        ));
        let State::Turn {
            activity, started, ..
        } = compact
        else {
            panic!()
        };
        assert_eq!(activity.as_str(), "COMPACTING");
        assert_eq!(started, at(1000));
        let idle = state(event("Interrupt", &Value::Null, Some(turn), at(6000)));
        assert!(matches!(idle, State::Ready { .. }));
        assert_eq!(idle.trailer().last, Some(Duration::from_millis(5000)));
    }

    #[test]
    fn pending_question_turn_finishes_but_question_survives_on_the_wire() {
        let turn = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        let question = state(event(
            "PreToolUse",
            &json!({"tool_name":"request_user_input_async"}),
            Some(turn),
            at(2000),
        ));
        let waiting = state(event("Stop", &Value::Null, Some(question), at(3500)));
        assert_eq!(waiting.to_line(), "input last 2500");
        let waiting = super::super::parse(&waiting.to_line(), at(3500), at(3500)).unwrap();
        for name in [
            "Stop",
            "PreToolUse",
            "PostToolUse",
            "PreCompact",
            "PostCompact",
        ] {
            assert_eq!(
                event(name, &Value::Null, Some(waiting.clone()), at(9000)),
                Action::Nothing
            );
        }
        assert!(matches!(
            event("UserPromptSubmit", &Value::Null, Some(waiting), at(10000)),
            Action::Set(State::Turn { .. }, _)
        ));
    }

    #[test]
    fn synchronous_question_is_not_a_permission_and_clears_on_return() {
        let turn = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        let question = state(event(
            "PreToolUse",
            &json!({"tool_name":"request_user_input"}),
            Some(turn),
            at(2000),
        ));
        assert_eq!(question.to_line(), "question started 1000");
        let question = super::super::parse(&question.to_line(), at(2000), at(2000)).unwrap();
        let resumed = state(event("PostToolUse", &Value::Null, Some(question), at(3000)));
        assert_eq!(resumed.to_line(), "turn THINKING started 1000");
    }

    #[test]
    fn permission_survives_a_feed_roundtrip_without_resetting_elapsed_time() {
        let s = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        let s = state(event("PermissionRequest", &Value::Null, Some(s), at(2000)));
        // This crosses the actual wire boundary between hook invocations. A bare
        // ASK used to discard the start and restart the clock after an approval.
        assert_eq!(s.to_line(), "review started 1000");
        let s = super::super::parse(&s.to_line(), at(2000), at(2000)).unwrap();
        let s = state(event("PostToolUse", &Value::Null, Some(s), at(8000)));
        let s = state(event("Stop", &Value::Null, Some(s), at(10000)));
        assert_eq!(s.trailer().last, Some(Duration::from_millis(9000)));
    }

    #[test]
    fn background_completion_does_not_resurrect_a_finished_parent() {
        let s = State::Ready {
            trailer: Trailer {
                agents: 1,
                last: Some(Duration::from_secs(3)),
                ..Default::default()
            },
        };
        let s = state(event("SubagentStop", &Value::Null, Some(s), at(5000)));
        assert!(matches!(s, State::Ready { .. }));
        assert_eq!(s.trailer().agents, 0);
        assert_eq!(s.trailer().last, Some(Duration::from_secs(3)));
    }

    #[test]
    fn generated_hooks_cover_the_supported_adapter_events() {
        let config: Value = serde_json::from_str(&hooks(false)).unwrap();
        let hooks = config["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), 11);
        for (name, groups) in hooks {
            let command = groups[0]["hooks"][0]["command"].as_str().unwrap();
            assert_eq!(command, format!("g13map agent codex {name}"));
            assert_eq!(groups[0]["hooks"][0]["timeout"], 3);
            assert_ne!(event(name, &Value::Null, None, at(1)), Action::Nothing);
        }
        let manual: Value = serde_json::from_str(&super::hooks(true)).unwrap();
        assert_eq!(
            manual["hooks"]["PermissionRequest"][0]["hooks"][0]["command"],
            "g13map agent codex HumanPermissionRequest"
        );
    }

    #[test]
    fn human_questions_and_remote_review_have_distinct_lifetimes() {
        let remote = state(event("PermissionRequest", &Value::Null, None, at(1)));
        assert!(matches!(
            remote,
            State::Ask {
                attention: Attention::Review,
                ..
            }
        ));
        let manual = state(event("HumanPermissionRequest", &Value::Null, None, at(1)));
        assert!(matches!(
            manual,
            State::Ask {
                attention: Attention::Permission,
                ..
            }
        ));
        let input = state(event(
            "PreToolUse",
            &json!({"tool_name":"request_user_input_async"}),
            None,
            at(2),
        ));
        assert!(matches!(
            input,
            State::Ask {
                attention: Attention::Question { asynchronous: true },
                ..
            }
        ));
        let input = super::super::parse(&input.to_line(), at(2), at(2)).unwrap();
        for name in ["PreToolUse", "PostToolUse", "PermissionRequest"] {
            assert_eq!(
                event(name, &Value::Null, Some(input.clone()), at(3)),
                Action::Nothing
            );
        }
        assert!(matches!(
            event("UserPromptSubmit", &Value::Null, Some(input), at(4)),
            Action::Set(State::Turn { .. }, _)
        ));
        let sync = state(event(
            "PreToolUse",
            &json!({"tool_name":"request_user_input"}),
            None,
            at(5),
        ));
        assert!(matches!(
            event("PostToolUse", &Value::Null, Some(sync), at(6)),
            Action::Set(State::Turn { .. }, _)
        ));
    }

    #[test]
    fn explicit_denial_overrides_attention_and_survives_stop() {
        let turn = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        let question = state(event(
            "PreToolUse",
            &serde_json::json!({"tool_name":"request_user_input_async"}),
            Some(turn),
            at(2000),
        ));
        let denied = state(event("Denied", &Value::Null, Some(question), at(2500)));
        assert_eq!(denied.to_line(), "denied last 1500 deny 2500");
        let denied = super::super::parse(&denied.to_line(), at(2500), at(2500)).unwrap();
        for name in ["Stop", "PostToolUse", "PreToolUse", "PermissionRequest"] {
            assert_eq!(
                event(name, &Value::Null, Some(denied.clone()), at(3000)),
                Action::Nothing
            );
        }
        assert!(matches!(
            event(
                "UserPromptSubmit",
                &Value::Null,
                Some(denied.clone()),
                at(4000)
            ),
            Action::Set(State::Turn { .. }, _)
        ));
        assert_eq!(
            event("SessionEnd", &Value::Null, Some(denied), at(5000)),
            Action::Clear
        );
    }

    #[test]
    fn varied_lifecycle_sequences_keep_counts_and_clock_across_the_wire() {
        let mut seed = super::super::epoch_ms(SystemTime::now()) | 1;
        let mut s = state(event("UserPromptSubmit", &Value::Null, None, at(1000)));
        let mut agents = 0u32;
        for tick in 1..=2000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let name = match (seed >> 32) % 5 {
                0 => {
                    agents = agents.saturating_add(1).min(99);
                    "SubagentStart"
                }
                1 => {
                    agents = agents.saturating_sub(1);
                    "SubagentStop"
                }
                2 => "PermissionRequest",
                3 => "PostToolUse",
                _ => "PreCompact",
            };
            let now = at(1000 + tick);
            s = state(event(name, &Value::Null, Some(s), now));
            assert_eq!(s.trailer().agents, agents, "event {name}, seed {seed}");
            let line = s.to_line();
            s = super::super::parse(&line, now, now).unwrap();
        }
        let s = state(event("Stop", &Value::Null, Some(s), at(9000)));
        assert_eq!(s.trailer().last, Some(Duration::from_millis(8000)));
    }
}
