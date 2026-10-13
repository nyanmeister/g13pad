// SPDX-License-Identifier: GPL-3.0-or-later
//! Rasterize each changed identity once in the headless font helper, then scroll
//! a tiny packed strip. The watcher never keeps an egui/font atlas resident.
use crate::lcd::{Bitmap, W};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
struct Strip {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}
struct State {
    wanted: String,
    ready: Option<(String, Strip)>,
    stop: bool,
}
pub(super) struct Rows {
    state: Arc<(Mutex<State>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Rows {
    pub fn start() -> Self {
        let state = Arc::new((
            Mutex::new(State {
                wanted: String::new(),
                ready: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let shared = state.clone();
        let worker = thread::spawn(move || {
            let mut previous = String::new();
            loop {
                let mut state = shared.0.lock().unwrap();
                while !state.stop && (state.wanted.is_empty() || state.wanted == previous) {
                    state = shared.1.wait(state).unwrap();
                }
                if state.stop {
                    break;
                }
                let text = state.wanted.clone();
                drop(state);
                let strip = rasterize(&text, &shared);
                let mut state = shared.0.lock().unwrap();
                if state.wanted == text {
                    state.ready = strip.map(|s| (text.clone(), s));
                    previous = text;
                }
            }
        });
        Self {
            state,
            worker: Some(worker),
        }
    }
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        self.state.0.lock().unwrap().ready.is_some()
    }
    pub fn request(&self, text: &str) {
        let mut state = self.state.0.lock().unwrap();
        if state.wanted != text {
            state.wanted = text.into();
            state.ready = None;
            self.state.1.notify_one();
        }
    }
    pub fn paint(&self, bm: &mut Bitmap, text: &str, phase: usize) {
        let state = self.state.0.lock().unwrap();
        if let Some((identity, strip)) = &state.ready {
            if identity == text {
                let span = strip.width + W / 2;
                let offset = if strip.width > W { phase % span } else { 0 };
                for y in 0..strip.height {
                    for x in 0..W {
                        let sx = (x + offset) % span;
                        if sx < strip.width {
                            let bit = y * strip.width + sx;
                            bm.set(x, y, strip.pixels[bit / 8] & (1 << (bit % 8)) != 0);
                        }
                    }
                }
                return;
            }
        }
        let chars = text
            .chars()
            .map(|c| {
                if c.is_ascii() {
                    c.to_ascii_uppercase()
                } else {
                    '?'
                }
            })
            .collect::<Vec<_>>();
        let width = chars.len() * 6;
        let offset = if width > W {
            phase % (width + W / 2)
        } else {
            0
        };
        for (i, c) in chars.into_iter().enumerate() {
            let x = i as i32 * 6 - offset as i32;
            if x > -6 && x < W as i32 {
                crate::meter::text(bm, x, 1, &c.to_string(), 1);
            }
        }
    }
}
impl Drop for Rows {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stop = true;
        self.state.1.notify_one();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn rasterize(text: &str, state: &Arc<(Mutex<State>, Condvar)>) -> Option<Strip> {
    if text.chars().count() > 256 {
        return None;
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    #[cfg(test)]
    let directory = if directory.file_name() == Some(std::ffi::OsStr::new("deps")) {
        directory.parent()?
    } else {
        directory
    };
    let helper = directory.join("g13map-editor");
    let helper = if helper.is_file() {
        helper
    } else {
        std::path::PathBuf::from("/usr/bin/g13map-editor")
    };
    let mut child = Command::new(helper)
        .arg("--lcd-row")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if child.stdin.take()?.write_all(text.as_bytes()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        let stale = {
            let s = state.0.lock().unwrap();
            s.stop || s.wanted != text
        };
        if stale || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()?
        .take(3211)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() < 10 || &bytes[..4] != b"MPR1" {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let height = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as usize;
    if width == 0
        || width > 2560
        || height == 0
        || height > 10
        || bytes.len() != 10 + (width * height).div_ceil(8)
    {
        return None;
    }
    Some(Strip {
        width,
        height,
        pixels: bytes[10..].to_vec(),
    })
}
