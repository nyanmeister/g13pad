// SPDX-License-Identifier: GPL-3.0-or-later
//! Runtime responsibilities for Health Mode.
use super::*;
use crate::display_clock::Clock;

// ---- the meter on the panel ----

/// What the frame thread follows: the state, the resting colour, and a request to send
/// the colour again after something else wrote the backlight.
pub(super) struct Shared {
    pub(super) state: State,
    pub(super) rest: [u8; 3],
    pub(super) resend: bool,
    pub(super) tuning: Tuning,
}

/// The meter running: a thread drawing a frame a tick and sending the backlight colour
/// the band calls for. Everything the pipe gets comes from that one thread, in order.
/// Dropping it stops the frames and puts the resting colour back; the caller puts the
/// picture back.
pub struct Live {
    pub(super) shared: Arc<Mutex<Shared>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) thread: Option<thread::JoinHandle<()>>,
    pub(super) reader: Reader,
    /// The CS2 listener, for `Reader::Cs2`; dropped with the meter.
    _listener: Option<Listener>,
}

impl Live {
    /// Starts the frames (yielding to an open editor, like a kept animation) and, for
    /// `Reader::Cs2`, the game-state listener.
    pub fn start(state: State, rest: [u8; 3], reader: Reader, profile: &str) -> Live {
        let tuning = Tuning::load_for(profile);
        let listener = match reader {
            Reader::Feed => None,
            Reader::Cs2 => Some(Listener::start(tuning.cs2_port)),
            Reader::Log => Some(Listener::follow(tuning.log_file.clone())),
            Reader::RetroArch | Reader::Dolphin => None,
        };
        let shared = Arc::new(Mutex::new(Shared {
            state,
            rest,
            resend: false,
            tuning,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            thread::spawn(move || match reader {
                Reader::RetroArch => crate::ra::run(&stop, crate::ra::Source::RetroArch),
                Reader::Dolphin => crate::ra::run(&stop, crate::ra::Source::Dolphin),
                _ => run(&shared, &stop),
            })
        };
        Live {
            reader,
            _listener: listener,
            shared,
            stop,
            thread: Some(thread),
        }
    }
    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn set(&mut self, state: State) {
        self.lock().state = state;
    }
    /// A new look, from the file; the frame thread takes it on its next tick.
    pub fn tune(&mut self, tuning: Tuning) {
        self.lock().tuning = tuning;
    }
    pub fn reader(&self) -> Reader {
        self.reader
    }
    /// A new resting colour (a profile switch); the switch wrote the backlight, so the
    /// current colour goes up again over it.
    pub fn rest(&mut self, rest: [u8; 3]) {
        let mut s = self.lock();
        s.rest = rest;
        s.resend = true;
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

pub(super) fn run(shared: &Mutex<Shared>, stop: &AtomicBool) {
    // The watcher joins the previous Live thread before starting another. Keep the
    // attack clock across those threads so leaving/re-entering a game profile
    // cannot bypass the six-second limit. Preview/test meters keep their own clocks.
    static LAST_ATTACK_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    static LAST_FORTRESS_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    let mut meter = Meter {
        attack_flash: *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()),
        ..Meter::default()
    };
    meter.fortress.last_flash = *LAST_FORTRESS_FLASH
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut clock = Clock::new(Instant::now());
    // The colour on the device: the profile's until the meter writes one. Waiting alone
    // never touches the backlight.
    let mut sent: Option<[u8; 3]> = None;
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        if crate::lcd::editor_open() {
            thread::sleep(Duration::from_millis(250));
            continue;
        }
        let now = Instant::now();
        if !clock.frame_due(now, TICK) {
            thread::sleep(clock.frame_wait(now));
            continue;
        }
        let (state, resend, rest) = {
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            let resend = std::mem::take(&mut s.resend);
            sent.get_or_insert(s.rest);
            if s.tuning != meter.tuning {
                meter.tuning = s.tuning.clone();
            }
            (s.state, resend, s.rest)
        };
        let frame = meter.frame(state);
        let want = meter.colour(state, rest);
        let colour = if sent != Some(want) || resend {
            let [r, g, b] = want;
            crate::daemon::send(&format!("rgb {r} {g} {b}\n")).map(|_| sent = Some(want))
        } else {
            Ok(())
        };
        if let Err(e) = colour.and_then(|_| crate::daemon::send_lcd(&frame.0)) {
            // The daemon is gone or not reading; say so once, try again in a while.
            if e != last_error {
                eprintln!("health meter: {e}");
                last_error = e;
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        thread::sleep(clock.frame_wait(Instant::now()));
    }
    *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()) = meter.attack_flash;
    *LAST_FORTRESS_FLASH
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = meter.fortress.last_flash;
    // The resting colour as it is now: the profile that ends the meter set it just
    // before stopping us (found 2026-10-07: the old profile's olive came back over
    // the new one's purple).
    let rest = shared.lock().unwrap_or_else(|e| e.into_inner()).rest;
    if sent.is_some_and(|c| c != rest) {
        let [r, g, b] = rest;
        if let Err(e) = crate::daemon::send(&format!("rgb {r} {g} {b}\n")) {
            eprintln!("health meter colour: {e}");
        }
    }
}
