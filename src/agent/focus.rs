// SPDX-License-Identifier: GPL-3.0-or-later
//! `g13map agent focus [next]`: raise the agent terminal that needs you, or cycle through
//! the live agent terminals. Meant for a run binding on a G-key (asked 2026-10-27).
//!
//! `focus` picks an agent waiting on a human first (a permission or a question; a remote
//! check is not for him), the longest-pending one; without any, the agent the pad shows:
//! the focused window's own feed when it has one, else the newest feed. `next` walks the
//! live feeds in window-id order from the focused window, wrapping; a focused window with
//! no feed starts at the first. Feeds are keyed by X11 window id, so this is X11 (and i3)
//! only, like the window rules.
use super::{feed, Attention, State};
use std::time::SystemTime;

type Live = (u64, SystemTime, State);

pub(crate) fn command(next: bool) -> Result<String, String> {
    let live = feed::read_all();
    if live.is_empty() {
        return Err("no live agent feed (g13map agent status)".into());
    }
    // The desktop is asked only once there is something to raise.
    let active = crate::focus::windows()
        .ok()
        .and_then(|ws| ws.into_iter().find(|w| w.focused).and_then(|w| w.window));
    let (win, _, state) = choose(&live, active, next).ok_or("no agent to focus")?;
    crate::focus::activate(*win)?;
    Ok(format!("focused window {win}: {}", state.to_line()))
}

/// The feed to raise; see the module docs. `None` only for an empty list.
pub(crate) fn choose(live: &[Live], active: Option<u64>, next: bool) -> Option<&Live> {
    let mut by_id: Vec<&Live> = live.iter().collect();
    by_id.sort_by_key(|f| f.0);
    if next {
        let at = active.and_then(|a| by_id.iter().position(|f| f.0 == a));
        return match at {
            Some(i) => Some(by_id[(i + 1) % by_id.len()]),
            None => by_id.first().copied(),
        };
    }
    let waiting = live
        .iter()
        .filter(|f| {
            matches!(
                f.2,
                State::Ask {
                    attention: Attention::Permission | Attention::Question { .. },
                    ..
                }
            )
        })
        .min_by_key(|f| f.1);
    if let Some(w) = waiting {
        return Some(w);
    }
    if let Some(own) = active.and_then(|a| live.iter().find(|f| f.0 == a)) {
        return Some(own);
    }
    live.iter().max_by_key(|f| f.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Activity, Trailer};
    use std::time::Duration;

    fn at(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(s)
    }
    fn turn() -> State {
        State::Turn {
            activity: Activity::parse("EDITING"),
            started: at(1),
            trailer: Trailer::default(),
        }
    }
    fn ask(attention: Attention) -> State {
        State::Ask {
            attention,
            started: None,
            trailer: Trailer::default(),
        }
    }

    #[test]
    fn focus_prefers_the_longest_waiting_human_ask() {
        // Hand-derived: 30 has asked since t=2000, 20 since t=3000, 10 is working, 40 is
        // waiting on a remote check (not a human). Whatever is focused, 30 comes first.
        let live = vec![
            (10, at(5000), turn()),
            (20, at(3000), ask(Attention::Permission)),
            (
                30,
                at(2000),
                ask(Attention::Question { asynchronous: true }),
            ),
            (40, at(1000), ask(Attention::Review)),
        ];
        for active in [None, Some(10), Some(20), Some(30), Some(77)] {
            assert_eq!(choose(&live, active, false).unwrap().0, 30, "{active:?}");
        }
        // No human ask: the focused window's own feed, else the newest feed (10 at t=5000).
        let quiet = vec![
            (10, at(5000), turn()),
            (20, at(3000), turn()),
            (40, at(1000), ask(Attention::Review)),
        ];
        assert_eq!(choose(&quiet, Some(20), false).unwrap().0, 20);
        assert_eq!(choose(&quiet, Some(77), false).unwrap().0, 10);
        assert_eq!(choose(&quiet, None, false).unwrap().0, 10);
        assert!(choose(&[], Some(10), false).is_none());
    }

    #[test]
    fn next_cycles_by_window_id_from_the_focused_one() {
        let live = vec![
            (30, at(1), turn()),
            (10, at(2), turn()),
            (20, at(3), ask(Attention::Permission)),
        ];
        assert_eq!(choose(&live, Some(10), true).unwrap().0, 20);
        assert_eq!(choose(&live, Some(20), true).unwrap().0, 30);
        assert_eq!(choose(&live, Some(30), true).unwrap().0, 10); // wraps
        assert_eq!(choose(&live, Some(77), true).unwrap().0, 10); // not an agent: the first
        assert_eq!(choose(&live, None, true).unwrap().0, 10);
        assert!(choose(&[], None, true).is_none());
    }
}
