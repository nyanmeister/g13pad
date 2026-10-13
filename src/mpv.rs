// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only mpv feed display. Playback and editor ownership stay independent.
use crate::lcd::{Bitmap, BYTES, W};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct Live {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Live {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::spawn(move || {
            let mut due = Instant::now();
            let mut frame = render(None);
            let mut shown = false;
            let mut refresh = true;
            let mut retry = Instant::now();
            let mut error = String::new();
            while !flag.load(Ordering::Relaxed) {
                let now = Instant::now();
                if now >= due {
                    frame =
                        render(read(&crate::state_dir().join("mpv"), SystemTime::now()).as_ref());
                    due = now + Duration::from_millis(250);
                    refresh = true;
                }
                if crate::lcd::editor_open() {
                    shown = false;
                } else if now >= retry && (refresh || !shown) {
                    match crate::daemon::send_lcd(&frame) {
                        Ok(()) => {
                            shown = true;
                            refresh = false;
                            error.clear();
                        }
                        Err(e) => {
                            if error != e {
                                eprintln!("mpv playback: {e}");
                                error = e;
                            }
                            retry = now + Duration::from_secs(1);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
        Self {
            stop,
            thread: Some(worker),
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

/// Prefer a focused player, then its most recent sample. Never replay expired feeds.
fn read(dir: &Path, now: SystemTime) -> Option<Value> {
    let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    let mut best: Option<(bool, f64, u64, Value)> = None;
    for entry in fs::read_dir(dir).ok()?.take(128).flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() || meta.len() > 16384 {
            continue;
        }
        let mut text = String::new();
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        if file.take(16385).read_to_string(&mut text).is_err() || text.len() > 16384 {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if value["version"].as_u64() != Some(1) {
            continue;
        }
        let Some(stamp) = number(&value, "written") else {
            continue;
        };
        if !(0.0..=3.0).contains(&(now - stamp)) {
            continue;
        }
        let pid = value["pid"].as_u64().unwrap_or(0);
        let focused = value["focused"].as_bool().unwrap_or(false);
        if best
            .as_ref()
            .is_none_or(|(f, t, p, _)| (focused, stamp, pid) > (*f, *t, *p))
        {
            best = Some((focused, stamp, pid, value));
        }
    }
    best.map(|(_, _, _, value)| value)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value[key].as_f64().filter(|n| n.is_finite())
}
pub fn preview(path: &Path) -> Result<String, String> {
    let frame = render(read(&crate::state_dir().join("mpv"), SystemTime::now()).as_ref());
    fs::write(path, frame).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("wrote mpv LCD preview to {}", path.display()))
}
fn label(value: &Value, key: &str, max: usize) -> String {
    value[key]
        .as_str()
        .unwrap_or("--")
        .chars()
        .take(max)
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-/.".contains(c) {
                c.to_ascii_uppercase()
            } else {
                '-'
            }
        })
        .collect()
}
fn count(value: &Value, key: &str) -> String {
    match value[key].as_u64() {
        Some(n) if n < 1000 => n.to_string(),
        Some(_) => "999+".into(),
        None => "--".into(),
    }
}
fn time(seconds: Option<f64>) -> String {
    let Some(s) = seconds.filter(|s| s.is_finite() && *s >= 0.0 && *s < 360000.0) else {
        return "--:--".into();
    };
    let s = s.floor() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
fn line(bm: &mut Bitmap, y: i32, text: &str) {
    let bounded: String = text.chars().take(26).collect();
    crate::meter::text(bm, 0, y, &bounded, 1);
}

/// Four fixed text rows; elapsed/remaining and the full-width bar never rotate away.
pub fn render(sample: Option<&Value>) -> [u8; BYTES] {
    let mut bm = Bitmap::blank();
    let empty = Value::Null;
    let v = sample.unwrap_or(&empty);
    let idle = v["idle"].as_bool().unwrap_or(sample.is_none());
    if sample.is_none() {
        line(&mut bm, 0, "MPV: WAITING FOR PLAYER");
        line(&mut bm, 8, "OPEN A VIDEO IN MPV");
    } else if idle {
        line(&mut bm, 0, "MPV: IDLE");
    } else {
        let height = number(v, "height")
            .filter(|h| *h > 0.0 && *h <= 16384.0)
            .map_or("--".into(), |h| format!("{h:.0}P"));
        let decoder = label(v, "hwdec", 10);
        let decoder = if decoder == "NO" { "SW" } else { &decoder };
        line(
            &mut bm,
            0,
            &format!("{} {height} {decoder}", label(v, "codec", 7)),
        );
        let sync = number(v, "avsync")
            .filter(|s| s.abs() < 10.0)
            .map_or("--".into(), |s| {
                let ms = (s * 1000.0).round();
                format!("{:+.0}", if ms == 0.0 { 0.0 } else { ms })
            });
        line(
            &mut bm,
            8,
            &format!(
                "D:{} V:{} AV:{sync}MS",
                count(v, "decoder_drops"),
                count(v, "display_drops")
            ),
        );
        let buffer = number(v, "buffer")
            .filter(|s| *s >= 0.0 && *s < 100000.0)
            .map_or("--".into(), |s| format!("{s:.0}"));
        let gpu = number(v, "render_ms")
            .filter(|s| *s >= 0.0 && *s < 1000.0)
            .map_or("--".into(), |s| format!("{s:.1}"));
        line(&mut bm, 16, &format!("BUF:{buffer}S GPU:{gpu}MS"));
        if v["paused"].as_bool() == Some(true) {
            for x in [0, 1, 4, 5] {
                for y in 24..28 {
                    bm.set(x, y, true);
                }
            }
        }
    }
    let position = (!idle)
        .then(|| number(v, "position"))
        .flatten()
        .filter(|p| *p >= 0.0);
    let duration = (!idle)
        .then(|| number(v, "duration"))
        .flatten()
        .filter(|d| *d > 0.0);
    let remaining = position.zip(duration).map(|(p, d)| (d - p).max(0.0));
    let elapsed = time(position);
    let remaining = format!("-{}", time(remaining));
    crate::meter::text(&mut bm, 0, 29, &elapsed, 1);
    crate::meter::text(
        &mut bm,
        (W as i32 - remaining.len() as i32 * 6 + 1).max(0),
        29,
        &remaining,
        1,
    );
    let fill = position
        .zip(duration)
        .map(|(p, d)| ((p / d).clamp(0.0, 1.0) * W as f64).round() as usize);
    for x in 0..W {
        bm.set(x, 40, true);
        if fill.is_some_and(|n| x < n) {
            for y in 38..43 {
                bm.set(x, y, true);
            }
        }
    }
    if fill.is_none() {
        for y in 38..43 {
            bm.set(0, y, true);
            bm.set(W - 1, y, true);
        }
    }
    bm.0.try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn progress_handles_seek_pause_end_and_unknown_duration() {
        let mut v = json!({"position": 25.0, "duration": 100.0, "paused": true});
        for (position, fill) in [(25.0, 40), (75.0, 120), (0.0, 0), (110.0, 160)] {
            v["position"] = json!(position);
            let bm = Bitmap(render(Some(&v)).to_vec());
            assert_eq!((0..W).filter(|x| bm.get(*x, 42)).count(), fill);
            assert!((0..W).all(|x| bm.get(x, 40)));
        }
        v["duration"] = Value::Null;
        assert_eq!(time(None), "--:--");
        assert_eq!(time(Some(3670.0)), "1:01:10");
        assert_eq!(time(Some(f64::INFINITY)), "--:--");
        let bm = Bitmap(render(Some(&v)).to_vec());
        assert!(!bm.get(80, 42));
    }
    #[test]
    fn feeds_expire_and_focused_player_wins_over_newer_background() {
        let sandbox = crate::test_support::Sandbox::new("mpv-feed");
        let dir = &sandbox.dir;
        let now = UNIX_EPOCH + Duration::from_secs(100);
        let focused = json!({"version":1,"pid":1,"written":99,"focused":true});
        fs::write(dir.join("1.json"), focused.to_string()).unwrap();
        fs::write(
            dir.join("2.json"),
            json!({"version":1,"pid":2,"written":100}).to_string(),
        )
        .unwrap();
        fs::write(dir.join("broken.json"), "broken").unwrap();
        assert_eq!(read(dir, now).unwrap()["pid"], 1);
        assert!(read(dir, now + Duration::from_secs(4)).is_none());
        fs::write(dir.join("1.json"), "{}".repeat(20000)).unwrap();
        assert_eq!(read(dir, now).unwrap()["pid"], 2);
    }
    #[test]
    fn editor_yields_without_stopping_playback_display() {
        let _sandbox = crate::test_support::Sandbox::new("mpv-editor");
        let pipe = crate::test_support::PanelPipe::new();
        let live = Live::start();
        assert_eq!(pipe.read().len(), BYTES);
        let editor = crate::lcd::editor_lock().expect("mpv held editor lock");
        thread::sleep(Duration::from_millis(100));
        pipe.expect_quiet();
        drop(editor);
        assert_eq!(pipe.read().len(), BYTES);
        drop(live);
        pipe.expect_quiet();
    }
}
