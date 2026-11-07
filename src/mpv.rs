// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only mpv feed display. Playback and editor ownership stay independent.
use crate::draw::Draw;
use crate::lcd::{Bitmap, BYTES, W};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The mpv screen: the newest player's JSON under the state dir, re-read every 250 ms.
pub fn start(rest: [u8; 3]) -> crate::screen::Live {
    let dir = crate::state_dir().join("mpv");
    crate::screen::Live::start(
        rest,
        Duration::from_millis(250),
        "mpv playback",
        move |_| {
            let frame = render(read(&dir, SystemTime::now()).as_ref());
            (Bitmap(frame.to_vec()), None)
        },
    )
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
    crate::meter::clock(s.floor() as u64)
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
            bm.fill(0, 24, 2, 4, true);
            bm.fill(4, 24, 2, 4, true);
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
    bm.line(0, 40, W as i32 - 1, 40);
    match fill {
        Some(n) => bm.fill(0, 38, n as i32, 5, true),
        None => {
            bm.fill(0, 38, 1, 5, true);
            bm.fill(W as i32 - 1, 38, 1, 5, true);
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
        let live = start([0, 0, 255]);
        assert_eq!(pipe.read().len(), BYTES);
        let editor = crate::lcd::editor_lock().expect("mpv held editor lock");
        std::thread::sleep(Duration::from_millis(100));
        pipe.expect_quiet();
        drop(editor);
        assert_eq!(pipe.read().len(), BYTES);
        drop(live);
        pipe.expect_quiet();
    }
}
