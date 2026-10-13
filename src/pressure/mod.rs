// SPDX-License-Identifier: GPL-3.0-or-later
//! Profile-selected System pressure display. The editor always has LCD priority.
pub mod data;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Live {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Live {
    pub fn start() -> Self {
        Self::start_from(PathBuf::from("/proc/pressure"))
    }
    fn start_from(root: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::spawn(move || run(&worker_stop, root));
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
fn run(stop: &AtomicBool, root: PathBuf) {
    let mut sampler = data::Sampler::new(root, Instant::now());
    let mut frame = sampler.frame();
    let mut next_sample = Instant::now() + Duration::from_secs(1);
    let mut retry_at = Instant::now();
    let mut shown = None;
    let mut refresh = true;
    let mut error = String::new();
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= next_sample {
            sampler.sample(now);
            frame = sampler.frame();
            // Also refresh unchanged values after a driver reconnect or explicit
            // profile re-send, without requiring a pressure counter to change.
            refresh = true;
            next_sample += Duration::from_secs(1);
            if next_sample <= now {
                next_sample = now + Duration::from_secs(1);
            }
        }
        if crate::lcd::editor_open() {
            // Do not own the editor's lock. Repaint promptly after it closes,
            // even if counters and the rendered frame have not changed.
            shown = None;
        } else if now >= retry_at && (refresh || shown.as_ref() != Some(&frame)) {
            match crate::daemon::send_lcd(&frame) {
                Ok(()) => {
                    shown = Some(frame);
                    refresh = false;
                    error.clear();
                }
                Err(e) => {
                    if error != e {
                        eprintln!("system pressure: {e}");
                        error = e;
                    }
                    retry_at = now + Duration::from_secs(1);
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_can_take_the_lock_and_pressure_resumes_after_it_closes() {
        let sandbox = crate::test_support::Sandbox::new("pressure-editor");
        let pipe = crate::test_support::PanelPipe::new();
        let root = sandbox.dir.join("psi");
        std::fs::create_dir_all(&root).unwrap();
        for name in ["cpu", "memory", "io"] {
            std::fs::write(root.join(name), "some avg10=0.00 total=100\n").unwrap();
        }
        let live = Live::start_from(root);
        assert_eq!(pipe.read().len(), crate::lcd::BYTES);
        let path = crate::daemon::runtime_file("editor").unwrap();
        let editor = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        editor
            .try_lock()
            .expect("System pressure blocked editor opening");
        thread::sleep(Duration::from_millis(100));
        pipe.expect_quiet();
        drop(editor);
        assert_eq!(pipe.read().len(), crate::lcd::BYTES);
        drop(live);
        pipe.expect_quiet();
    }
}
