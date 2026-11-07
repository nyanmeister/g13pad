// SPDX-License-Identifier: GPL-3.0-or-later
//! The agent screen on the glass: a state the watcher keeps setting (from the agents'
//! feed files), painted by the shared screen loop with the age of the current activity.
use super::{render, State};
use std::{
    mem::discriminant,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime},
};

const TICK: Duration = Duration::from_millis(20);

struct Shared {
    state: State,
    since: Instant,
}

pub struct Live {
    shared: Arc<Mutex<Shared>>,
    screen: crate::screen::Live,
}

impl Live {
    pub fn start(state: State, rest: [u8; 3]) -> Live {
        let shared = Arc::new(Mutex::new(Shared {
            state,
            since: Instant::now(),
        }));
        let painted = shared.clone();
        let screen = crate::screen::Live::start(rest, TICK, "agent screen", move |rest| {
            let (state, age) = {
                let s = painted.lock().unwrap_or_else(|e| e.into_inner());
                (s.state.clone(), s.since.elapsed())
            };
            let wall = SystemTime::now();
            (
                render::frame(&state, age, wall),
                Some(render::colour(&state, age, wall, rest)),
            )
        });
        Live { shared, screen }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&mut self, state: State) {
        let mut s = self.lock();
        if restart_age(&s.state, &state) {
            s.since = Instant::now();
        }
        s.state = state;
    }

    pub fn rest(&mut self, rest: [u8; 3]) {
        self.screen.rest(rest);
    }
}

/// Whether a new state restarts the activity stopwatch: a different ask, a different
/// activity or start, or a different kind of state; a heartbeat update does not.
fn restart_age(old: &State, new: &State) -> bool {
    match (old, new) {
        (State::Ask { attention: a, .. }, State::Ask { attention: b, .. }) => a != b,
        (
            State::Turn {
                activity: a,
                started: a_start,
                ..
            },
            State::Turn {
                activity: b,
                started: b_start,
                ..
            },
        ) => a != b || a_start != b_start,
        _ => discriminant(old) != discriminant(new),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Activity, Trailer};

    #[test]
    fn activity_clock_restarts_for_new_content_but_not_heartbeat_updates() {
        let old = State::Turn {
            activity: Activity::parse("LONG TOOL NAME"),
            started: SystemTime::UNIX_EPOCH,
            trailer: Trailer::default(),
        };
        assert!(!restart_age(&old, &old));
        let mut new = old.clone();
        if let State::Turn { trailer, .. } = &mut new {
            trailer.agents = 2;
        }
        assert!(!restart_age(&old, &new));
        if let State::Turn { activity, .. } = &mut new {
            *activity = Activity::parse("ANOTHER TOOL");
        }
        assert!(restart_age(&old, &new));
        new = old.clone();
        if let State::Turn { started, .. } = &mut new {
            *started += Duration::from_secs(1);
        }
        assert!(restart_age(&old, &new));
        assert!(restart_age(
            &old,
            &State::Ask {
                attention: crate::agent::Attention::Permission,
                started: None,
                trailer: Trailer::default()
            }
        ));
    }
}
