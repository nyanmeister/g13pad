// SPDX-License-Identifier: GPL-3.0-or-later
//! Runtime responsibilities for Health Mode.
use super::*;

// ---- the meter on the panel ----

/// What the frame thread follows: the state, the resting colour, and a request to send
/// the colour again after something else wrote the backlight.
pub(super) struct Shared {
    pub(super) state: State,
    pub(super) tuning: Tuning,
}

/// The health meter on the glass: a feed state the watcher keeps setting, painted by the
/// shared screen loop; the RetroAchievements readers run their own loop (`ra::run`).
pub struct Live {
    pub(super) shared: Arc<Mutex<Shared>>,
    screen: Screen,
    pub(super) reader: Reader,
    _listener: Option<Listener>,
}

enum Screen {
    Meter(crate::screen::Live),
    Ra {
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    },
}

impl Live {
    pub fn start(state: State, rest: [u8; 3], reader: Reader, profile: &str) -> Live {
        let tuning = Tuning::load_for(profile);
        let listener = match reader {
            Reader::Feed => None,
            Reader::Cs2 => Some(Listener::start(tuning.cs2_port)),
            Reader::Log => Some(Listener::follow(tuning.log_file.clone())),
            Reader::RetroArch | Reader::Dolphin => None,
        };
        let shared = Arc::new(Mutex::new(Shared { state, tuning }));
        let screen = match reader {
            Reader::RetroArch | Reader::Dolphin => {
                let source = if reader == Reader::RetroArch {
                    crate::ra::Source::RetroArch
                } else {
                    crate::ra::Source::Dolphin
                };
                let stop = Arc::new(AtomicBool::new(false));
                let flag = stop.clone();
                Screen::Ra {
                    stop,
                    thread: Some(thread::spawn(move || crate::ra::run(&flag, source))),
                }
            }
            _ => Screen::Meter(crate::screen::Live::start(
                rest,
                TICK,
                "health meter",
                painter(shared.clone()),
            )),
        };
        Live {
            shared,
            screen,
            reader,
            _listener: listener,
        }
    }
    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn set(&mut self, state: State) {
        self.lock().state = state;
    }
    pub fn tune(&mut self, tuning: Tuning) {
        self.lock().tuning = tuning;
    }
    pub fn reader(&self) -> Reader {
        self.reader
    }
    pub fn rest(&mut self, rest: [u8; 3]) {
        if let Screen::Meter(live) = &mut self.screen {
            live.rest(rest);
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if let Screen::Ra { stop, thread } = self {
            stop.store(true, Ordering::Relaxed);
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
        }
    }
}

/// The meter's paint for the screen loop. The attack and fortress flash instants outlive a
/// restart (a profile switch mid-fight must not re-flash), so they live in statics.
fn painter(shared: Arc<Mutex<Shared>>) -> impl FnMut([u8; 3]) -> crate::screen::Painted {
    static LAST_ATTACK_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    static LAST_FORTRESS_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    let mut meter = Meter {
        attack_flash: *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()),
        ..Meter::default()
    };
    meter.fortress.last_flash = *LAST_FORTRESS_FLASH
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    move |rest| {
        let state = {
            let s = shared.lock().unwrap_or_else(|e| e.into_inner());
            if s.tuning != meter.tuning {
                meter.tuning = s.tuning.clone();
            }
            s.state
        };
        let frame = meter.frame(state);
        let colour = meter.colour(state, rest);
        *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()) = meter.attack_flash;
        *LAST_FORTRESS_FLASH
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = meter.fortress.last_flash;
        (frame, Some(colour))
    }
}
