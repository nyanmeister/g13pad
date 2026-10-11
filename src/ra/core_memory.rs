// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded read-only core-memory transport over private owned pipes.
//!
//! RetroArch game adapters require a pipe owned at emulator launch. We cannot
//! attach stdin/stdout to an already-running, unwrapped emulator. Use the shared
//! G13 launcher (also suitable for a normal desktop entry); stock RetroArch's
//! existing command support needs no patched executable or plugin. Each game
//! still needs a verified layout and a core exposing its memory descriptors.
use super::{adapters, event, process_start, Event};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const INTERVAL: Duration = Duration::from_millis(200);
pub(super) struct Recipe {
    pub(super) file: &'static str,
    pub(super) collect: fn(&mut ChildStdin, &Receiver<String>) -> Option<Option<serde_json::Value>>,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Selection {
    game: Option<u32>,
    epoch: u64,
}
struct Control {
    selection: Mutex<Selection>,
    stop: AtomicBool,
    dir: PathBuf,
}
impl Control {
    fn select(&self, game: Option<u32>) {
        let mut current = self.selection.lock().unwrap();
        if let Some(recipe) = adapters::memory_recipe(current.game) {
            let _ = std::fs::remove_file(self.dir.join(recipe.file));
        }
        *current = Selection {
            game,
            epoch: current.epoch.wrapping_add(1),
        };
    }
    fn observe(&self, line: &str) {
        match event(line) {
            Some(Event::Identity(id, _) | Event::Loaded(id, _)) => self.select(Some(id)),
            Some(Event::Unload) => self.select(None),
            _ => {
                if line.starts_with("[INFO] [Core] Loading dynamic libretro core from:")
                    || line == "[INFO] [Core] Unloading game..."
                    || line == "[INFO] [Core] Unloading core..."
                {
                    self.select(None);
                } else if let Some(path) = line
                    .strip_prefix("[INFO] [Content] Loading content file: \"")
                    .and_then(|s| s.strip_suffix("\"."))
                {
                    // File signatures allow offline direct loads without an RA
                    // login. An RA identity, when available, supersedes this hint.
                    self.select(None);
                    self.select(adapters::content_game(Path::new(path)));
                }
            }
        }
    }
    fn publish(
        &self,
        sampled: Selection,
        recipe: &Recipe,
        value: Option<serde_json::Value>,
    ) -> bool {
        let current = self.selection.lock().unwrap();
        if *current != sampled {
            // The game changed during the reads, even if its ID is unchanged.
            return true;
        }
        let path = self.dir.join(recipe.file);
        if let Some(value) = value {
            let temp = path.with_extension("new");
            std::fs::write(&temp, value.to_string())
                .and_then(|()| std::fs::rename(temp, path))
                .is_ok()
        } else {
            let _ = std::fs::remove_file(path);
            true
        }
    }
}

pub(super) struct Capture {
    control: Arc<Control>,
    reader: JoinHandle<()>,
    poller: JoinHandle<()>,
}
impl Capture {
    pub(super) fn observe(&self, line: &str) {
        self.control.observe(line);
    }
    pub(super) fn finish(self) {
        self.control.stop.store(true, Ordering::Relaxed);
        let _ = self.reader.join();
        let _ = self.poller.join();
    }
}

// RetroArch accepts a pipe-separated append list; a repeated option replaces it.
pub(super) fn append_config(args: &[&str], private: &Path) -> Result<String, String> {
    let private = private.to_str().ok_or("memory config path is not UTF-8")?;
    if private.contains('|') {
        return Err("memory config path contains RetroArch's | separator".into());
    }
    let mut previous = None;
    for (i, arg) in args.iter().enumerate() {
        if *arg == "--appendconfig" {
            previous = Some(*args.get(i + 1).ok_or("--appendconfig requires a path")?);
        }
        if let Some(value) = arg.strip_prefix("--appendconfig=") {
            previous = Some(value);
        }
    }
    Ok(previous
        .filter(|s| !s.is_empty())
        .map_or_else(|| private.to_owned(), |s| format!("{s}|{private}")))
}

pub(super) fn bytes(line: &str, address: u32, size: usize) -> Option<Vec<u8>> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "READ_CORE_MEMORY"
        || u32::from_str_radix(fields.next()?, 16).ok()? != address
    {
        return None;
    }
    let mut result = Vec::with_capacity(size);
    for value in fields {
        if value.len() != 2 || result.len() == size {
            return None;
        }
        result.push(u8::from_str_radix(value, 16).ok()?);
    }
    (result.len() == size).then_some(result)
}
fn request(input: &mut ChildStdin, replies: &Receiver<String>, command: &str) -> Option<String> {
    writeln!(input, "{command}").ok()?;
    input.flush().ok()?;
    // Initial content/core loading can take longer than the LCD freshness TTL.
    replies.recv_timeout(Duration::from_secs(10)).ok()
}

pub(super) fn read_memory(
    input: &mut ChildStdin,
    replies: &Receiver<String>,
    address: u32,
    size: usize,
) -> Option<Option<Vec<u8>>> {
    if !(1..=256).contains(&size) {
        return None;
    }
    let reply = request(
        input,
        replies,
        &format!("READ_CORE_MEMORY {address:x} {size}"),
    )?;
    Some(bytes(&reply, address, size))
}
pub(super) fn capture(child: &mut Child, dir: PathBuf) -> Capture {
    let pid = child.id();
    let start = process_start(pid);
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(8);
    let reader = thread::spawn(move || {
        let mut pending = Vec::new();
        let mut overflow = false;
        let mut buf = [0; 4096];
        loop {
            let n = match output.read(&mut buf) {
                Ok(0) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
                Ok(n) => n,
            };
            for &b in &buf[..n] {
                if b == b'\n' {
                    if !overflow {
                        if let Ok(line) = std::str::from_utf8(&pending) {
                            if line.starts_with("READ_CORE_MEMORY ") {
                                let _ = sender.try_send(line.to_owned());
                            } else {
                                let _ = writeln!(std::io::stdout(), "{line}");
                            }
                        }
                    }
                    pending.clear();
                    overflow = false;
                } else if !overflow && pending.len() < 1024 {
                    pending.push(b);
                } else {
                    pending.clear();
                    overflow = true;
                }
            }
        }
    });
    let control = Arc::new(Control {
        selection: Mutex::new(Selection::default()),
        stop: AtomicBool::new(false),
        dir,
    });
    let polling = control.clone();
    let poller = thread::spawn(move || {
        while !polling.stop.load(Ordering::Relaxed)
            && process_start(pid).as_ref() == start.as_ref()
            && start.is_some()
        {
            let sampled = *polling.selection.lock().unwrap();
            if let Some(recipe) = adapters::memory_recipe(sampled.game) {
                let sampled_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|t| u64::try_from(t.as_millis()).ok());
                let Some(value) = (recipe.collect)(&mut input, &receiver) else {
                    break;
                };
                let value = value.map(|mut value| {
                    value["pid"] = pid.into();
                    value["start"] = serde_json::json!(start);
                    value["sampled_at"] = serde_json::json!(sampled_at);
                    value
                });
                if !polling.publish(sampled, recipe, value) {
                    break;
                }
            }
            thread::sleep(INTERVAL);
        }
        polling.select(None);
    });
    Capture {
        control,
        reader,
        poller,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_and_same_game_reload_discard_in_flight_samples() {
        let sandbox = crate::test_support::Sandbox::new("memory-switch");
        let control = Control {
            selection: Mutex::new(Selection::default()),
            stop: AtomicBool::new(false),
            dir: sandbox.dir.clone(),
        };
        let recipe = adapters::memory_recipe(Some(super::super::bn2::GAME)).unwrap();
        let path = sandbox.dir.join(recipe.file);
        control.observe("[INFO] [RCHEEVOS] Identified game: 773 \"BN2\"");
        let first = *control.selection.lock().unwrap();
        assert!(control.publish(first, recipe, Some(serde_json::json!({"sample":1}))));
        assert!(path.exists());
        control.observe("[INFO] [RCHEEVOS] Game 773 loaded, hardcore disabled");
        assert!(!path.exists());
        assert!(control.publish(first, recipe, Some(serde_json::json!({"stale":true}))));
        assert!(!path.exists());
        let reloaded = *control.selection.lock().unwrap();
        assert!(control.publish(reloaded, recipe, Some(serde_json::json!({"sample":2}))));
        assert!(path.exists());
        control.observe("[INFO] [RCHEEVOS] Identified game: 1 \"OTHER\"");
        assert!(!path.exists());
        assert!(adapters::memory_recipe(control.selection.lock().unwrap().game).is_none());
        assert!(control.publish(reloaded, recipe, Some(serde_json::json!({"stale":true}))));
        assert!(!path.exists());
        control.observe("[INFO] [RCHEEVOS] Identified game: 773 \"BN2\"");
        assert!(adapters::memory_recipe(control.selection.lock().unwrap().game).is_some());
        control.observe("[INFO] [Core] Unloading game...");
        assert!(adapters::memory_recipe(control.selection.lock().unwrap().game).is_none());
    }

    #[test]
    fn owned_pipe_is_idle_for_unknown_games_and_reactivates_after_switching() {
        let sandbox = crate::test_support::Sandbox::new("memory-pipe");
        let requests = sandbox.dir.join("requests");
        // A real child and pipes exercise the protocol boundary without a core.
        // An unavailable memory map replies ERROR; this must keep the transport
        // usable for the next detected game rather than end its collector.
        struct FixtureChild(Child);
        impl Drop for FixtureChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = FixtureChild(std::process::Command::new("sh")
            .arg("-c")
            .arg("while IFS= read -r request; do printf '%s\\n' \"$request\" >> \"$1\"; printf 'READ_CORE_MEMORY %s ERROR\\n' \"${request#READ_CORE_MEMORY }\"; done")
            .arg("memory-fixture")
            .arg(&requests)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap());
        let capture = capture(&mut child.0, sandbox.dir.clone());
        thread::sleep(INTERVAL * 2);
        assert!(!requests.exists());
        capture.observe("[INFO] [RCHEEVOS] Identified game: 1 \"OTHER\"");
        thread::sleep(INTERVAL * 2);
        assert!(!requests.exists());
        capture.observe("[INFO] [RCHEEVOS] Identified game: 773 \"BN2\"");
        let count = || {
            std::fs::read_to_string(&requests)
                .unwrap_or_default()
                .lines()
                .count()
        };
        let wait = |previous| {
            let until = std::time::Instant::now() + Duration::from_secs(3);
            while count() <= previous && std::time::Instant::now() < until {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(count() > previous);
        };
        wait(0);
        capture.observe("[INFO] [Core] Unloading game...");
        thread::sleep(INTERVAL * 2);
        let stopped = count();
        thread::sleep(INTERVAL * 2);
        assert_eq!(count(), stopped);
        capture.observe("[INFO] [RCHEEVOS] Identified game: 773 \"BN2\"");
        wait(stopped);
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        capture.finish();
        assert!(!sandbox.dir.join("bn2.json").exists());
    }
}
