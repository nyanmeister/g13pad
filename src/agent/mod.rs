// SPDX-License-Identifier: GPL-3.0-or-later
//! The "agent" screen: what the coding assistant (Claude Code, Codex, ...) is doing this
//! turn, read from a one-line feed. Not a game and not health — a live screen like
//! `mpv`/`pressure`, selected per profile and composed from the shared kit. The feeder
//! (the harness's hooks + status line) is per-harness; this grammar and the page are not,
//! so a Claude Code feeder and a Codex feeder write the same lines and the page neither
//! knows nor cares which wrote them.
//!
//! Its reason to exist is the looked-away read the terminal cannot give: the backlight
//! alerts (permission, failure, denial, idle) you would miss while glancing elsewhere, the
//! background-agent count the scrollback buries, and the frozen-pulse "it is hung" tell.
//! Context % and model/effort stay off it — they are already on the kitty status line.
mod codex;
pub mod feed;
pub mod feeder;
pub mod render;
pub mod runtime;
pub use feed::{parse, read, read_all, read_for, select, write};
pub use runtime::Live;

use std::time::{Duration, SystemTime};

/// Longest activity label the panel keeps; longer feeds are truncated, not rejected.
pub const MAX_ACTIVITY: usize = 64;
/// Most background agents the count shows (it is a glance, not a census).
pub const MAX_AGENTS: u32 = 99;

/// Unix-epoch milliseconds of `t`, the feed's wire form for an instant.
pub(crate) fn epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// The activity face: a short, upper-case, panel-renderable label (`EDITING`,
/// `RUNNING BASH`, `WEB SEARCH`). A feeder sends one token with underscores for the spaces.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Activity(String);

impl Activity {
    /// Clean a feed token into a renderable label: underscores become spaces, upper-cased,
    /// only glyphs the panel has (A–Z, 0–9, space), bounded. Nothing renderable means the
    /// turn has no tool yet, which reads as `THINKING` rather than a reject — the activity
    /// is cosmetic, so a surprising token degrades to a word, it does not kill the line.
    pub fn parse(word: &str) -> Activity {
        let cleaned: String = word
            .replace('_', " ")
            .to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == ' ')
            .collect();
        let trimmed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        let bounded: String = trimmed.chars().take(MAX_ACTIVITY).collect();
        Activity(if bounded.is_empty() {
            "THINKING".to_string()
        } else {
            bounded
        })
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// The single-token wire form: spaces back to underscores.
    pub fn token(&self) -> String {
        self.0.replace(' ', "_")
    }
}

/// What rides on every phase: the background-agent count, the previous turn's length, and
/// the timestamps of the last tool failure / edit denial (brief colour flashes the page
/// shows for a fixed window after each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Trailer {
    pub agents: u32,
    pub last: Option<Duration>,
    pub fail_at: Option<SystemTime>,
    pub deny_at: Option<SystemTime>,
}

/// Who needs attention: a human permission, a pending question, or remote review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    Permission,
    /// An asynchronous question stays pending across tools and turn completion.
    Question {
        asynchronous: bool,
    },
    Review,
}

/// Where the turn stands. Failure/denial timestamps remain independent of the phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// A turn in progress: the activity face and when it began (the page ticks the
    /// stopwatch from `started`, so it advances between feed reads without re-reading).
    Turn {
        activity: Activity,
        started: SystemTime,
        trailer: Trailer,
    },
    /// Attention needed. Questions keep the turn colour; no start means it finished.
    Ask {
        attention: Attention,
        started: Option<SystemTime>,
        trailer: Trailer,
    },
    /// The turn ended (the Stop hook): green, blinks twice, then holds.
    Ready { trailer: Trailer },
    /// A reported rejection: red, three blinks, then held until the next user prompt.
    Denied { trailer: Trailer },
    /// Idle-waiting past the threshold: green, the backlight breathes.
    Idle { trailer: Trailer },
    /// The profile is up but nothing is feeding yet (no agent running, or between turns
    /// with no ttl): a quiet placeholder, the profile's own backlight. Like the meter's
    /// `Wait`. The watcher shows this when the feed reads empty.
    Waiting { trailer: Trailer },
}

impl State {
    pub fn trailer(&self) -> Trailer {
        match self {
            State::Turn { trailer, .. }
            | State::Ask { trailer, .. }
            | State::Ready { trailer }
            | State::Denied { trailer }
            | State::Idle { trailer }
            | State::Waiting { trailer } => *trailer,
        }
    }

    /// The feed line for this state, without the trailing `ttl` (which `write` appends).
    pub fn to_line(&self) -> String {
        let t = self.trailer();
        let mut s = match self {
            State::Turn {
                activity, started, ..
            } => format!("turn {} started {}", activity.token(), epoch_ms(*started)),
            State::Ask {
                attention, started, ..
            } => {
                let word = match attention {
                    Attention::Permission => "ask",
                    Attention::Question { asynchronous: true } => "input",
                    Attention::Question {
                        asynchronous: false,
                    } => "question",
                    Attention::Review => "review",
                };
                started
                    .map(|t| format!("{word} started {}", epoch_ms(t)))
                    .unwrap_or_else(|| word.to_string())
            }
            State::Ready { .. } => "ready".to_string(),
            State::Denied { .. } => "denied".to_string(),
            State::Idle { .. } => "idle".to_string(),
            State::Waiting { .. } => "wait".to_string(),
        };
        if t.agents > 0 {
            s.push_str(&format!(" agents {}", t.agents));
        }
        if let Some(d) = t.last {
            s.push_str(&format!(" last {}", d.as_millis().min(u64::MAX as u128)));
        }
        if let Some(f) = t.fail_at {
            s.push_str(&format!(" fail {}", epoch_ms(f)));
        }
        if let Some(d) = t.deny_at {
            s.push_str(&format!(" deny {}", epoch_ms(d)));
        }
        s
    }
}
