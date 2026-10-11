// SPDX-License-Identifier: GPL-3.0-or-later
//! Session-bound sampling and independently scheduled RetroAchievements presentation.
use super::*;
use crate::display_clock::Clock;

pub(crate) fn run(stop: &AtomicBool, source: Source) {
    let mut model = Model::default();
    let mut display = Display::default();
    let mut tail = Tail::default();
    let mut path = None;
    let mut native: Option<Session> = None;
    let mut scan_at = Instant::now();
    let mut error = String::new();
    let mut reader = adapters::Reader::default();
    let mut snapshot: Option<adapters::BoundSnapshot> = None;
    let mut observed = None;
    let mut epoch = 0u64;
    let mut fallback = None;
    let mut clock = Clock::new(Instant::now());
    let mut shown = None;
    let mut refresh = true;
    while !stop.load(Ordering::Relaxed) {
        if clock.sample_due(Instant::now()) {
            refresh = true;
            let current = (source == Source::RetroArch)
                .then(session)
                .flatten()
                .or_else(|| {
                    if native.as_ref().is_some_and(|s| {
                        process_start(s.pid).as_deref() != Some(&s.start)
                            || !native_log_fd(&s.path, source)
                    }) {
                        native = None;
                    }
                    if Instant::now() >= scan_at {
                        native = native_session(Path::new("/proc"), source);
                        scan_at = Instant::now() + Duration::from_secs(2);
                    }
                    native.clone()
                });
            if current != path {
                model = Model::default();
                display = Display::default();
                tail = Tail::default();
                path = current;
                reader = adapters::Reader::default();
            }
            if let Some(p) = &path {
                if tail.poll(&p.path, &mut model, source, p.since).is_err() {
                    model = Model::default();
                    tail = Tail::default();
                }
            }
            let identity = (
                path.clone(),
                model.game,
                model.generation,
                model.memory_epoch,
            );
            if observed.as_ref() != Some(&identity) {
                epoch = epoch.wrapping_add(1);
                observed = Some(identity);
            }
            // Bootstrap never publishes Dolphin telemetry from old append-only records.
            snapshot = if source == Source::Dolphin && tail.bootstrap {
                None
            } else {
                match reader.poll(path.as_ref(), source, model.game, epoch) {
                    adapters::Outcome::Ready(value) => {
                        fallback = None;
                        Some(value)
                    }
                    adapters::Outcome::Fallback(reason) => {
                        if fallback != Some(reason) && reason != adapters::Fallback::NoAdapter {
                            eprintln!("{} telemetry: {reason:?}", source.name());
                        }
                        fallback = Some(reason);
                        None
                    }
                }
            };
        }
        let now = Instant::now();
        if clock.frame_due(now, display.frame_interval()) && !crate::lcd::editor_open() {
            let frame = if source == Source::Dolphin && tail.bootstrap {
                Display::default().frame_for(&mut Model::default(), now, source)
            } else {
                let view = snapshot
                    .as_ref()
                    .filter(|s| {
                        path.as_ref().is_some_and(|session| {
                            s.binding.matches(session, source, model.game, epoch)
                        })
                    })
                    .map(adapters::BoundSnapshot::view);
                display.frame_adapter(&mut model, now, source, view)
            };
            // Keep the usual 100 ms refresh for profile handoffs, but avoid
            // sending identical intermediate animation frames over USB.
            if refresh || shown.as_ref() != Some(&frame) {
                match crate::daemon::send_lcd(&frame.0) {
                    Ok(()) => {
                        shown = Some(frame);
                        refresh = false;
                    }
                    Err(e) if e != error => {
                        eprintln!("{} LCD: {e}", source.name());
                        error = e;
                    }
                    Err(_) => {}
                }
            }
        }
        thread::sleep(clock.wait(Instant::now()));
    }
}
