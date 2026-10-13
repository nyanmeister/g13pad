// SPDX-License-Identifier: GPL-3.0-or-later
//! Now-playing presentation. Acquisition outlives profile visibility; no controls.
use super::{mpd_send_status, Client, Error};
use crate::{
    audio::MusicPeaks,
    lcd::{Bitmap, BYTES, W},
};
use serde_json::Value;
use std::{
    ffi::c_void,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};
mod title;
#[link(name = "mpdclient")]
unsafe extern "C" {
    fn mpd_send_current_song(p: *mut c_void) -> bool;
}
fn value<'a>(p: &'a [(String, String)], key: &str) -> Option<&'a str> {
    p.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}
fn number(p: &[(String, String)], key: &str) -> Option<f64> {
    value(p, key)?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(256).collect()
}
#[derive(Clone, Default, Debug)]
struct Track {
    id: Option<u32>,
    uri: String,
    identity: String,
    playing: bool,
    paused: bool,
    elapsed: Option<f64>,
    duration: Option<f64>,
    count: Option<u64>,
    last: Option<u64>,
}
impl Track {
    fn metadata(status: &[(String, String)], song: &[(String, String)]) -> Result<Self, Error> {
        let id = value(status, "songid").and_then(|v| v.parse().ok());
        let song_id = value(song, "Id").and_then(|v| v.parse().ok());
        if id != song_id {
            return Err(Error::Offline);
        }
        let uri = value(song, "file").unwrap_or("").to_owned();
        let identity = if uri.is_empty() {
            "MPD: STOPPED".into()
        } else {
            let filename = uri.rsplit('/').next().unwrap_or(&uri);
            let fallback = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
            format!(
                "{} - {}",
                clean(
                    value(song, "Artist")
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Unknown artist")
                ),
                clean(
                    value(song, "Title")
                        .filter(|s| !s.is_empty())
                        .unwrap_or(fallback)
                )
            )
        };
        Ok(Self {
            id,
            uri,
            identity: identity.chars().take(256).collect(),
            playing: value(status, "state") == Some("play"),
            paused: value(status, "state") == Some("pause"),
            elapsed: number(status, "elapsed"),
            duration: number(status, "duration")
                .or_else(|| number(song, "duration"))
                .filter(|n| *n > 0.),
            ..Default::default()
        })
    }
    fn history(&mut self, endpoint: &str, response: &Value) {
        // A previous song's snapshot, or another MPD endpoint, must not leak counts.
        self.count = None;
        self.last = None;
        if response["uri"].as_str() != Some(&self.uri)
            || response["endpoint"].as_str() != Some(endpoint)
        {
            return;
        }
        self.count = response["playcount"].as_u64();
        self.last = response["lastplayed"].as_u64().filter(|n| *n > 0);
    }
}
fn history(track: &mut Track, endpoint: &str) {
    let data = g13pad_music::request_timeout("track", Duration::from_millis(300))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    track.history(endpoint, data.as_ref().unwrap_or(&Value::Null));
}
fn time(value: Option<f64>) -> String {
    value
        .filter(|v| v.is_finite() && *v >= 0. && *v <= 999_999.)
        .map_or("--:--".into(), |s| {
            let s = s.floor() as u64;
            if s >= 3600 {
                format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
            } else {
                format!("{}:{:02}", s / 60, s % 60)
            }
        })
}
fn compact(n: u64) -> String {
    for (size, suffix) in [
        (1_000_000_000_000_000_000, "E"),
        (1_000_000_000_000_000, "Q"),
        (1_000_000_000_000, "T"),
        (1_000_000_000, "B"),
        (1_000_000, "M"),
        (1000, "K"),
    ] {
        if n >= size {
            return format!("{:.1}{suffix}", n as f64 / size as f64);
        }
    }
    n.to_string()
}
fn history_line(track: &Track) -> String {
    let n = track.count.map_or("--".into(), |n| n.to_string());
    let noun = if track.count == Some(1) {
        "play"
    } else {
        "plays"
    };
    let date = track
        .last
        .and_then(g13pad_music::local_date)
        .unwrap_or_else(|| {
            if track.count == Some(0) {
                "never".into()
            } else {
                "--".into()
            }
        });
    let text = format!("{n} {noun}, last {date}");
    if text.len() <= 26 {
        text
    } else {
        format!(
            "{} {noun}, {date}",
            track.count.map_or("--".into(), compact)
        )
    }
}
#[derive(Clone, PartialEq)]
struct Graph {
    values: [Option<f32>; W],
    next: usize,
}
impl Default for Graph {
    fn default() -> Self {
        Self {
            values: [None; W],
            next: 0,
        }
    }
}
impl Graph {
    fn push(&mut self, playing: bool, peak: Option<f32>) {
        if playing {
            self.values[self.next] = peak
                .filter(|p| p.is_finite() && *p >= 0.)
                .map(|p| p.min(1.));
            self.next = (self.next + 1) % W;
        }
    }
    fn paint(&self, bm: &mut Bitmap) {
        for x in 0..W {
            if x % 4 == 0 {
                bm.set(x, 35, true);
            }
            if let Some(p) = self.values[(self.next + x) % W] {
                let db = if p > 0.001 {
                    (20. * p.log10()).max(-60.)
                } else {
                    -60.
                };
                let height = (((db + 60.) / 60.) * 10.).round() as usize;
                for y in (42 - height)..43 {
                    bm.set(x, y, true);
                }
                if p >= 0.999 {
                    bm.set(x, 31, true);
                }
            }
        }
    }
}
fn render(
    track: Result<&Track, Error>,
    graph: &Graph,
    phase: usize,
    title: &title::Rows,
) -> [u8; BYTES] {
    let mut bm = Bitmap::blank();
    match track {
        Err(e) => {
            crate::meter::text(&mut bm, 0, 2, e.label(), 1);
            crate::meter::text(&mut bm, 0, 12, "WAITING FOR DAEMON", 1);
        }
        Ok(t) => {
            title.paint(&mut bm, &t.identity, phase);
            crate::meter::text(&mut bm, 0, 11, &history_line(t).to_ascii_uppercase(), 1);
            let elapsed = time(t.elapsed);
            let remaining = format!(
                "-{}",
                time(t.elapsed.zip(t.duration).map(|(p, d)| (d - p).max(0.)))
            );
            crate::meter::text(&mut bm, 0, 20, &elapsed, 1);
            crate::meter::text(
                &mut bm,
                (W as i32 - remaining.len() as i32 * 6 + 1).max(0),
                20,
                &remaining,
                1,
            );
            if t.paused {
                for x in [77, 78, 81, 82] {
                    for y in 21..26 {
                        bm.set(x, y, true);
                    }
                }
            }
            let fill = t
                .elapsed
                .zip(t.duration)
                .map(|(p, d)| ((p / d).clamp(0., 1.) * W as f64).round() as usize);
            for x in 0..W {
                bm.set(x, 29, true);
                if fill.is_some_and(|n| x < n) {
                    bm.set(x, 28, true);
                }
            }
            graph.paint(&mut bm);
        }
    }
    bm.0.try_into().unwrap()
}
fn local(endpoint: &str) -> bool {
    endpoint.starts_with("unix:")
        || endpoint
            .strip_prefix("tcp:")
            .and_then(|s| s.rsplit_once(':'))
            .and_then(|(host, _)| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback())
}
enum Message {
    Display(bool, SyncSender<()>),
    Stop,
}
pub struct Live {
    tx: SyncSender<Message>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Live {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel(8);
        let worker = thread::spawn(move || run(rx));
        Self {
            tx,
            worker: Some(worker),
        }
    }
    /// Ack after the old frame has retired; then the next profile owns the LCD.
    pub fn display(&self, on: bool) {
        let (tx, rx) = mpsc::sync_channel(0);
        if self.tx.send(Message::Display(on, tx)).is_ok() {
            let _ = rx.recv();
        }
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Stop);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn run(rx: Receiver<Message>) {
    let endpoint = g13pad_music::daemon_endpoint().unwrap_or_default();
    let peaks = MusicPeaks::start();
    let title = title::Rows::start();
    let mut graph = Graph::default();
    let mut client = None;
    let mut daemon_pid = None;
    let mut track: Result<Track, Error> = Err(Error::Offline);
    let mut song = Vec::new();
    let mut visible = true;
    let start = Instant::now();
    let mut due = start;
    let mut metadata_due = start;
    let mut history_due = start;
    let mut graph_due = start;
    let mut shown = None;
    let mut last_refresh = start;
    let mut write_retry = start;
    loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Message::Display(on, ack)) => {
                visible = on;
                shown = None;
                let _ = ack.send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if now >= due {
            let result = (|| {
                if client.is_none() {
                    let connected = Client::connect()?;
                    let pid = connected.process_id();
                    if daemon_pid.is_some() && pid.is_some() && daemon_pid != pid {
                        graph = Graph::default();
                    }
                    if pid.is_some() {
                        daemon_pid = pid;
                    }
                    client = Some(connected);
                    metadata_due = now;
                    history_due = now;
                }
                let c = client.as_mut().unwrap();
                let status = c.pairs(mpd_send_status)?;
                let id = value(&status, "songid").and_then(|v| v.parse::<u32>().ok());
                if now >= metadata_due || track.as_ref().ok().map(|t| t.id) != Some(id) {
                    song = c.pairs(mpd_send_current_song)?;
                    metadata_due = now + Duration::from_secs(1);
                }
                let mut next = match Track::metadata(&status, &song) {
                    Ok(next) => next,
                    Err(_) => {
                        let status = c.pairs(mpd_send_status)?;
                        song = c.pairs(mpd_send_current_song)?;
                        Track::metadata(&status, &song)?
                    }
                };
                if let Ok(old) = &track {
                    if old.uri == next.uri {
                        next.count = old.count;
                        next.last = old.last;
                    }
                }
                if now >= history_due || track.as_ref().ok().is_none_or(|t| t.uri != next.uri) {
                    history(&mut next, &endpoint);
                    history_due = Instant::now() + Duration::from_secs(1);
                }
                Ok(next)
            })();
            if result.is_err() {
                client = None;
                song.clear();
                // Keep the ring through a brief reconnect; PID changes retire it.
            }
            track = result;
            due = Instant::now()
                + if client.is_some() {
                    Duration::from_millis(200)
                } else {
                    Duration::from_secs(2)
                };
            if let Ok(t) = &track {
                title.request(&t.identity);
            }
            peaks.enable_process(
                local(&endpoint) && track.as_ref().is_ok_and(|t| t.playing || t.paused),
                client.as_ref().and_then(Client::process_id),
            );
        }
        let now = Instant::now();
        if now >= graph_due {
            graph.push(track.as_ref().is_ok_and(|t| t.playing), peaks.sample());
            graph_due = now + Duration::from_millis(40);
        }
        if !visible {
            continue;
        }
        if crate::lcd::editor_open() {
            shown = None;
            continue;
        }
        let frame = render(
            track.as_ref().map_err(|e| *e),
            &graph,
            (start.elapsed().as_millis() / 50) as usize,
            &title,
        );
        if (shown != Some(frame) || last_refresh.elapsed() >= Duration::from_secs(2))
            && now >= write_retry
        {
            match crate::daemon::send_lcd(&frame) {
                Ok(()) => {
                    shown = Some(frame);
                    last_refresh = now;
                }
                Err(_) => write_retry = now + Duration::from_secs(1),
            }
        }
    }
}
pub(super) fn preview(file: &str) -> Result<String, String> {
    let mut client = Client::connect().map_err(|e| e.label().to_owned())?;
    let status = client
        .pairs(mpd_send_status)
        .map_err(|e| e.label().to_owned())?;
    let song = client
        .pairs(mpd_send_current_song)
        .map_err(|e| e.label().to_owned())?;
    let mut track = Track::metadata(&status, &song).map_err(|e| e.label().to_owned())?;
    history(&mut track, &g13pad_music::daemon_endpoint()?);
    let title = title::Rows::start();
    title.request(&track.identity);
    let peaks = MusicPeaks::start();
    peaks.enable_process(
        track.playing && local(&g13pad_music::daemon_endpoint()?),
        client.process_id(),
    );
    let mut graph = Graph::default();
    for _ in 0..25 {
        thread::sleep(Duration::from_millis(40));
        graph.push(track.playing, peaks.sample());
    }
    std::fs::write(file, render(Ok(&track), &graph, 0, &title)).map_err(|e| e.to_string())?;
    Ok(format!("wrote MPD playback LCD preview to {file}"))
}
#[cfg(test)]
mod tests {

    #[test]
    #[ignore = "writes synthetic MPD screen examples to G13MAP_MPD_DUMP (no live audio/MPD)"]
    fn dump_now_playing_frames() {
        let dir = std::env::var("G13MAP_MPD_DUMP").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for (name, identity, count, paused, phase) in [
            ("regular", "Artist - Title", 23, false, 0),
            ("singular", "Artist - Title", 1, true, 0),
            ("new", "Unknown artist - new track", 0, false, 0),
            (
                "japanese",
                "流線形 & 一十三十一 - エデンの惑星",
                23,
                false,
                0,
            ),
            ("long", "Donna Burke - Sins of The Father", 1000, false, 110),
        ] {
            let t = Track {
                identity: identity.into(),
                count: Some(count),
                last: (count != 0).then_some(1_789_905_600),
                elapsed: Some(46.),
                duration: Some(184.),
                paused,
                playing: !paused,
                ..Default::default()
            };
            let title = title::Rows::start();
            title.request(identity);
            for _ in 0..200 {
                if title.ready() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            assert!(title.ready(), "headless row helper failed for {name}");
            let mut graph = Graph::default();
            for x in 0..160 {
                let p = (((x as f32) * 0.08).sin() * 0.45 + 0.5).powi(4);
                graph.push(true, Some(p));
            }
            let bytes = render(Ok(&t), &graph, phase, &title);
            std::fs::write(format!("{dir}/{name}.lpbm"), bytes).unwrap();
            let bmp = Bitmap::from_lpbm(&bytes).unwrap();
            let mut pgm = b"P5\n160 43\n255\n".to_vec();
            pgm.extend_from_slice(&bmp.gray());
            std::fs::write(format!("{dir}/{name}.pgm"), pgm).unwrap();
        }
    }

    use super::*;
    #[test]
    fn history_grammar_dates_large_counts_and_mismatched_snapshots() {
        let mut t = Track {
            uri: "x".into(),
            count: Some(1),
            ..Default::default()
        };
        assert_eq!(history_line(&t), "1 play, last --");
        t.count = Some(0);
        assert_eq!(history_line(&t), "0 plays, last never");
        t.count = Some(u64::MAX);
        t.last = Some(1_789_900_000);
        assert!(history_line(&t).len() <= 26);
        t.history(
            "unix:a",
            &serde_json::json!({"uri":"y","endpoint":"unix:a","playcount":23,"lastplayed":0}),
        );
        assert_eq!(t.count, None);
        t.history(
            "unix:a",
            &serde_json::json!({"uri":"x","endpoint":"unix:b","playcount":23}),
        );
        assert_eq!(t.count, None);
        t.history(
            "unix:a",
            &serde_json::json!({"uri":"x","endpoint":"unix:a","playcount":23,"lastplayed":0}),
        );
        assert_eq!(t.count, Some(23));
    }
    #[test]
    fn graph_freezes_and_resumes_without_losing_history() {
        let mut g = Graph::default();
        for i in 0..180 {
            g.push(true, Some(i as f32 / 180.));
        }
        let held = g.clone();
        for _ in 0..100 {
            g.push(false, None);
        }
        assert!(g == held);
        g.push(true, Some(0.5));
        assert!(g != held);
        let mut bm = Bitmap::blank();
        g.paint(&mut bm);
        assert!((0..W).any(|x| bm.get(x, 40)));
    }
    #[test]
    fn track_metadata_and_time_have_honest_missing_values() {
        let status = [
            ("songid".into(), "3".into()),
            ("state".into(), "pause".into()),
            ("elapsed".into(), "46.8".into()),
            ("duration".into(), "184".into()),
        ];
        let song = [
            ("Id".into(), "3".into()),
            ("file".into(), "test/a.opus".into()),
            ("Artist".into(), "Artist".into()),
            ("Title".into(), "Title".into()),
        ];
        let t = Track::metadata(&status, &song).unwrap();
        assert_eq!(t.identity, "Artist - Title");
        assert!(t.paused && !t.playing);
        assert_eq!(time(t.elapsed), "0:46");
        assert_eq!(time(t.elapsed.zip(t.duration).map(|(p, d)| d - p)), "2:17");
        assert_eq!(time(Some(f64::NAN)), "--:--");
        assert_eq!(time(Some(3601.)), "1:00:01");
        assert!(Track::metadata(&status, &[]).is_err());
    }
}
