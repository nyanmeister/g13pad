// SPDX-License-Identifier: GPL-3.0-or-later
//! One loop for every live LCD screen. A screen is a thread that paints a frame on its
//! tick and sends it only when it changed, sends the backlight only when it changed,
//! yields to the editor (and resends its frame when the editor closes), says a send error
//! once and retries each second, and puts the profile's resting colour back when it stops.
//! The screens (meter, agent, combat, ship, alert, mpv, pressure) differ only in what they
//! paint and how often; that is the `paint` closure and the `tick`.
//!
//! A frame that did not change is not sent: the daemon's pipe is a device write, and a
//! static screen at 50 frames a second was most of the watcher's idle cost (2026-11-07).
//! Resends after a demo or an overlay come from the watcher through `rest()`, which also
//! carries a new resting colour. The worker parks between ticks and is unparked to stop.
use crate::{display_clock::Clock, lcd::Bitmap};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Live {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<(Rgb, bool)>>,
    thread: Option<thread::JoinHandle<()>>,
}

pub type Rgb = [u8; 3];

/// What one tick paints: the frame, and the backlight it wants (`None` leaves the
/// backlight as it is).
pub type Painted = (Bitmap, Option<Rgb>);

impl Live {
    /// `name` labels errors; `paint` gets the profile's resting colour each tick.
    pub fn start(
        rest: Rgb,
        tick: Duration,
        name: &'static str,
        mut paint: impl FnMut(Rgb) -> Painted + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new((rest, true)));
        let (stop_worker, shared_worker) = (stop.clone(), shared.clone());
        let thread = thread::spawn(move || {
            let mut clock = Clock::new(Instant::now());
            let mut shown: Option<Bitmap> = None;
            let mut sent: Option<Rgb> = None;
            let mut last_error = String::new();
            while !stop_worker.load(Ordering::Relaxed) {
                if crate::lcd::editor_open() {
                    shown = None; // the editor drew over us: our next frame goes up again
                    thread::park_timeout(Duration::from_millis(250));
                    continue;
                }
                let now = Instant::now();
                if !clock.frame_due(now, tick) {
                    thread::park_timeout(clock.frame_wait(now));
                    continue;
                }
                let (rest, resend) = {
                    let mut guard = shared_worker.lock().unwrap_or_else(|e| e.into_inner());
                    (guard.0, std::mem::take(&mut guard.1))
                };
                let (frame, colour) = paint(rest);
                let result = (|| {
                    if let Some(want) = colour {
                        if sent != Some(want) || resend {
                            let [r, g, b] = want;
                            crate::daemon::send(&format!("rgb {r} {g} {b}\n"))?;
                            sent = Some(want);
                        }
                    }
                    if resend || shown.as_ref() != Some(&frame) {
                        crate::daemon::send_lcd(&frame.0)?;
                        shown = Some(frame);
                    }
                    Ok::<_, String>(())
                })();
                if let Err(e) = result {
                    if e != last_error {
                        eprintln!("{name}: {e}");
                        last_error = e;
                    }
                    thread::park_timeout(Duration::from_secs(1));
                    continue;
                }
                thread::park_timeout(clock.frame_wait(Instant::now()));
            }
            let rest = shared_worker.lock().unwrap_or_else(|e| e.into_inner()).0;
            if sent.is_some_and(|c| c != rest) {
                let [r, g, b] = rest;
                if let Err(e) = crate::daemon::send(&format!("rgb {r} {g} {b}\n")) {
                    eprintln!("{name} colour: {e}");
                }
            }
        });
        Self {
            stop,
            shared,
            thread: Some(thread),
        }
    }
    /// A new resting colour, and a resend of the frame and backlight on the next tick.
    pub fn rest(&mut self, rest: Rgb) {
        *self.shared.lock().unwrap_or_else(|e| e.into_inner()) = (rest, true);
    }
}

impl Drop for Live {
    /// Stops at once: the worker parks between ticks (a second for the slowest screen) and
    /// is unparked here, so a display handoff never waits out a tick.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// A screen fed from a file another program writes (combat, ship, alert): paint reads
/// the feed each tick, with the profile's meter tuning for colours.
pub fn fed(
    rest: Rgb,
    profile: &str,
    name: &'static str,
    paint: fn(&crate::meter::Tuning, Rgb) -> (Bitmap, Rgb),
) -> Live {
    let tuning = crate::meter::Tuning::load_for(profile);
    Live::start(rest, Duration::from_millis(100), name, move |rest| {
        let (frame, colour) = paint(&tuning, rest);
        (frame, Some(colour))
    })
}
