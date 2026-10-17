// SPDX-License-Identifier: GPL-3.0-or-later
//! The agent screen running: a thread that draws a frame a tick and sends the backlight the
//! phase calls for, exactly like the health meter's `Live`, but with no reader to own — the
//! watcher polls `feed::read` and hands the state in through `set`. Dropping it stops the
//! frames and puts the resting colour back; the caller restores the picture.
use super::{render, State};
use crate::display_clock::Clock;
use std::{
    mem::discriminant,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

/// A tick: 50 a second, like the meter, so the pulse and the hundredths are smooth.
const TICK: Duration = Duration::from_millis(20);

struct Shared {
    state: State,
    /// When the phase or turn activity began, for blink/breath and marquee timing.
    since: Instant,
    rest: [u8; 3],
    resend: bool,
}

pub struct Live {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Live {
    pub fn start(state: State, rest: [u8; 3]) -> Live {
        let shared = Arc::new(Mutex::new(Shared {
            state,
            since: Instant::now(),
            rest,
            resend: false,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            thread::spawn(move || run(&shared, &stop))
        };
        Live {
            shared,
            stop,
            thread: Some(thread),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Restart the marquee on a new turn/activity, and the alerts on a new phase.
    /// Trailer-only updates do not restart either clock.
    pub fn set(&mut self, state: State) {
        let mut s = self.lock();
        if restart_age(&s.state, &state) {
            s.since = Instant::now();
        }
        s.state = state;
    }

    /// A new resting colour (a profile switch wrote the backlight): send ours again over it.
    pub fn rest(&mut self, rest: [u8; 3]) {
        let mut s = self.lock();
        s.rest = rest;
        s.resend = true;
    }
}

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

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let mut clock = Clock::new(Instant::now());
    // The colour on the device: none until we write one, so the profile's own stands until
    // the first frame (a quiet Waiting leaves the backlight alone, like the meter's wait).
    let mut sent: Option<[u8; 3]> = None;
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        if crate::lcd::editor_open() {
            thread::sleep(Duration::from_millis(250));
            continue;
        }
        let now = Instant::now();
        if !clock.frame_due(now, TICK) {
            // frame_wait, not wait: this thread never advances the sample clock, so wait()
            // (min of sample/frame) would return 0 and spin the thread, hammering the LCD
            // lock and starving profile-switch writes. The meter paces the same way.
            thread::sleep(clock.frame_wait(now));
            continue;
        }
        let (state, age, resend, rest) = {
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            let resend = std::mem::take(&mut s.resend);
            sent.get_or_insert(s.rest);
            (s.state.clone(), s.since.elapsed(), resend, s.rest)
        };
        let wall = SystemTime::now();
        let frame = render::frame(&state, age, wall);
        let want = render::colour(&state, age, wall, rest);
        let colour = if sent != Some(want) || resend {
            let [r, g, b] = want;
            crate::daemon::send(&format!("rgb {r} {g} {b}\n")).map(|_| sent = Some(want))
        } else {
            Ok(())
        };
        if let Err(e) = colour.and_then(|_| crate::daemon::send_lcd(&frame.0)) {
            if e != last_error {
                eprintln!("agent screen: {e}");
                last_error = e;
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        thread::sleep(clock.frame_wait(Instant::now()));
    }
    // Put the resting colour back as it is now (the ending profile set it just before
    // stopping us), the same handoff the meter does.
    let rest = shared.lock().unwrap_or_else(|e| e.into_inner()).rest;
    if sent.is_some_and(|c| c != rest) {
        let [r, g, b] = rest;
        if let Err(e) = crate::daemon::send(&format!("rgb {r} {g} {b}\n")) {
            eprintln!("agent screen colour: {e}");
        }
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
