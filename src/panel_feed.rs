// SPDX-License-Identifier: GPL-3.0-or-later
//! Static telemetry pages: poll cheaply and send only changed pixels.
use crate::lcd::Bitmap;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub struct Live {
    stop: Arc<AtomicBool>,
    rest: Arc<Mutex<([u8; 3], bool)>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Live {
    pub fn start(
        rest: [u8; 3],
        profile: &str,
        paint: fn(&crate::meter::Tuning, [u8; 3]) -> (Bitmap, [u8; 3]),
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new((rest, true)));
        let (stop_worker, rest_worker) = (stop.clone(), shared.clone());
        let tuning = crate::meter::Tuning::load_for(profile);
        let thread = thread::spawn(move || {
            let mut previous = None;
            let mut sent = None;
            let mut first = true;
            let mut error = String::new();
            while !stop_worker.load(Ordering::Relaxed) {
                if !crate::lcd::editor_open() {
                    let (rest, resend) = {
                        let mut guard = rest_worker.lock().unwrap_or_else(|e| e.into_inner());
                        (guard.0, std::mem::take(&mut guard.1))
                    };
                    let (next, colour) = paint(&tuning, rest);
                    let result = (|| {
                        if sent != Some(colour) || resend {
                            let [r, g, b] = colour;
                            crate::daemon::send(&format!("rgb {r} {g} {b}\n"))?;
                            sent = Some(colour);
                        }
                        // The chosen page is static: no 50fps redraw of identical pixels.
                        if first || previous.as_ref() != Some(&next) || resend {
                            crate::daemon::send_lcd(&next.0)?;
                            previous = Some(next);
                            first = false;
                        }
                        Ok::<_, String>(())
                    })();
                    if let Err(e) = result {
                        if e != error {
                            eprintln!("panel feed: {e}");
                            error = e;
                        }
                    }
                } else {
                    first = true;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let rest = rest_worker.lock().unwrap_or_else(|e| e.into_inner()).0;
            if sent.is_some_and(|c| c != rest) {
                let [r, g, b] = rest;
                let _ = crate::daemon::send(&format!("rgb {r} {g} {b}\n"));
            }
        });
        Self {
            stop,
            rest: shared,
            thread: Some(thread),
        }
    }
    pub fn rest(&mut self, rest: [u8; 3]) {
        *self.rest.lock().unwrap_or_else(|e| e.into_inner()) = (rest, true);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
