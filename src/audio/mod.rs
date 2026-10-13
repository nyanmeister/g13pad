// SPDX-License-Identifier: GPL-3.0-or-later
//! System-wide audio controls and selected-input peak history. No PCM is retained.
mod pulse;
use crate::lcd::{Bitmap, BYTES, W};
use std::{
    fs,
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Input {
    pub name: String,
    pub description: String,
    pub monitor: bool,
}
impl Input {
    pub fn label(&self) -> String {
        let label = if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        };
        if self.monitor {
            format!("{label} (output monitor)")
        } else {
            label.clone()
        }
    }
}
#[derive(Default)]
pub struct Inputs {
    pub devices: Vec<Input>,
    pub default: Option<String>,
}
/// A one-shot device query. Never opens a level stream or changes desktop defaults.
pub fn inputs() -> Result<Inputs, String> {
    pulse::Client::connect(None)?.inputs()
}
fn profile_microphone() -> Option<String> {
    crate::load(&crate::active_name())
        .ok()
        .and_then(|(p, _)| p.audio_mic)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    MicToggle,
    MicMute,
    MicUnmute,
    MicUp,
    MicDown,
    OutputToggle,
    VolumeUp,
    VolumeDown,
}
impl Action {
    pub const ALL: [Self; 8] = [
        Self::MicToggle,
        Self::MicMute,
        Self::MicUnmute,
        Self::MicUp,
        Self::MicDown,
        Self::OutputToggle,
        Self::VolumeUp,
        Self::VolumeDown,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::MicToggle => "mic-toggle",
            Self::MicMute => "mic-mute",
            Self::MicUnmute => "mic-unmute",
            Self::MicUp => "mic-up",
            Self::MicDown => "mic-down",
            Self::OutputToggle => "output-toggle",
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::MicToggle => "Mic mute / unmute",
            Self::MicMute => "Mute microphone",
            Self::MicUnmute => "Unmute microphone",
            Self::MicUp => "Mic level +5%",
            Self::MicDown => "Mic level -5%",
            Self::OutputToggle => "Output mute / unmute",
            Self::VolumeUp => "Listening volume +5%",
            Self::VolumeDown => "Listening volume -5%",
        }
    }
    pub fn binding(self) -> String {
        format!(">AUDIO:{};", self.key())
    }
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }
    pub fn from_binding(text: &str) -> Option<Self> {
        Self::parse(text.strip_prefix(">AUDIO:")?.strip_suffix(';')?)
    }
    pub(super) fn microphone(self) -> bool {
        matches!(
            self,
            Self::MicToggle | Self::MicMute | Self::MicUnmute | Self::MicUp | Self::MicDown
        )
    }
}

#[derive(Clone, Default)]
struct Sample {
    source: Option<pulse::Device>,
    sink: Option<pulse::Device>,
    peak: Option<f32>,
}
impl Sample {
    fn colour(&self) -> [u8; 3] {
        match self.source.as_ref().map(|d| d.muted) {
            Some(true) => [255, 0, 0],
            Some(false) => [0, 255, 0],
            None => [255, 0, 0],
        }
    }
    fn identity(&self) -> Option<&str> {
        self.source.as_ref().map(|d| d.name.as_str())
    }
}
const PERIOD: Duration = Duration::from_millis(40);
//160samples at25Hz:6.4seconds. None is a gap, distinct from measured silence.
struct History {
    values: [Option<f32>; W],
    next: usize,
}
impl Default for History {
    fn default() -> Self {
        Self {
            values: [None; W],
            next: 0,
        }
    }
}
impl History {
    fn push(&mut self, peak: Option<f32>) {
        self.values[self.next] = peak;
        self.next = (self.next + 1) % W;
    }
    fn at(&self, x: usize) -> Option<f32> {
        self.values[(self.next + x) % W]
    }
}
fn text(bm: &mut Bitmap, y: i32, s: &str) {
    let s: String = s
        .chars()
        .take(26)
        .map(|c| {
            if c.is_ascii_alphanumeric() || " -+:%./".contains(c) {
                c.to_ascii_uppercase()
            } else {
                '-'
            }
        })
        .collect();
    crate::meter::text(bm, 0, y, &s, 1);
}
fn dbfs(peak: f32) -> f32 {
    if peak > 0.001 {
        (20.0 * peak.min(1.0).log10()).max(-60.0)
    } else {
        -60.0
    }
}
fn render(sample: &Sample, history: &History) -> [u8; BYTES] {
    let mut bm = Bitmap::blank();
    let mic = match &sample.source {
        Some(d) => {
            if d.muted {
                "MIC MUTED"
            } else {
                "MIC OPEN"
            }
        }
        None => "MIC UNAVAILABLE",
    };
    let input = sample
        .source
        .as_ref()
        .map(|d| format!("{}%", d.percent()))
        .unwrap_or("--".into());
    text(&mut bm, 0, &format!("{mic} {input}"));
    let output = sample
        .sink
        .as_ref()
        .map(|d| {
            format!(
                "OUT {} {}%",
                if d.muted { "MUTED" } else { "OPEN" },
                d.percent()
            )
        })
        .unwrap_or("OUT UNAVAILABLE".into());
    let level = sample
        .peak
        .map(|p| format!("{:.0}", dbfs(p)))
        .unwrap_or("--".into());
    text(&mut bm, 8, &format!("{output} {level}DB"));
    text(
        &mut bm,
        16,
        sample
            .source
            .as_ref()
            .map(|d| d.description.as_str())
            .unwrap_or("CHECK DESKTOP AUDIO"),
    );
    for x in 0..W {
        // Fixed-60..0dBFS scale, with dotted-20dB reference and a clip marker.
        if x % 4 == 0 {
            bm.set(x, 30, true);
        }
        if let Some(peak) = history.at(x) {
            let height = (((dbfs(peak) + 60.0) / 60.0) * 16.0).round() as usize;
            for y in (42 - height)..43 {
                bm.set(x, y, true);
            }
            if peak >= 0.999 {
                bm.set(x, 24, true);
            }
        }
    }
    bm.0.try_into().unwrap()
}

enum Message {
    Display(Option<[u8; 3]>, Option<String>, SyncSender<()>),
    Action(Action, Option<String>),
    Stop,
}
pub struct Live {
    tx: SyncSender<Message>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Default for Live {
    fn default() -> Self {
        Self::start()
    }
}
impl Live {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel(32);
        Self {
            tx,
            worker: Some(thread::spawn(move || run(rx))),
        }
    }
    pub fn display(&self, rest: Option<[u8; 3]>, microphone: Option<String>) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.tx.send(Message::Display(rest, microphone, tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    }
    pub fn action(&self, action: Action, microphone: Option<String>) {
        if let Err(e) = self.tx.try_send(Message::Action(action, microphone)) {
            eprintln!("system audio control: {e}");
        }
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn run(rx: Receiver<Message>) {
    let mut display = None;
    let mut microphone = None;
    let mut client: Option<pulse::Client> = None;
    let mut sample = Sample::default();
    let mut history = History::default();
    let mut identity = String::new();
    let mut shown = None;
    let mut colour = None;
    let mut retry = Instant::now();
    let mut write_retry = Instant::now();
    let mut last_sample = Instant::now();
    let mut last_refresh = Instant::now();
    let mut error = String::new();
    loop {
        match rx.recv_timeout(if display.is_some() {
            Duration::from_millis(20)
        } else {
            Duration::from_secs(60)
        }) {
            Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Message::Display(rest, selected, ack)) => {
                display = rest;
                shown = None;
                colour = None;
                if display.is_none() || selected != microphone {
                    client = None;
                    sample = Sample::default();
                    history = History::default();
                    identity.clear();
                    retry = Instant::now();
                }
                microphone = selected;
                let _ = ack.send(());
            }
            Ok(Message::Action(action, selected)) => {
                if microphone != selected {
                    microphone = selected;
                    client = None;
                    sample = Sample::default();
                    history = History::default();
                    identity.clear();
                    shown = None;
                    colour = None;
                    retry = Instant::now();
                }
                let result = (|| {
                    if client.is_none() {
                        client = Some(pulse::Client::connect(microphone.as_deref())?);
                    }
                    client.as_mut().unwrap().perform(action)
                })();
                if let Err(e) = result {
                    eprintln!("system audio {}: {e}", action.key());
                }
                if display.is_none() {
                    client = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if display.is_none() {
            continue;
        }
        let now = Instant::now();
        if client.is_none() && now >= retry {
            match pulse::Client::connect(microphone.as_deref()) {
                Ok(c) => {
                    client = Some(c);
                    error.clear();
                }
                Err(e) => {
                    if error != e {
                        eprintln!("system audio: {e}");
                        error = e;
                    }
                    retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        if let Some(c) = &mut client {
            match c.tick(true) {
                Ok(()) => {
                    let (source, sink) = c.devices();
                    sample = Sample {
                        source,
                        sink,
                        peak: c.peak(),
                    };
                }
                Err(e) => {
                    if error != e {
                        eprintln!("system audio: {e}");
                        error = e;
                    }
                    client = None;
                    sample = Sample::default();
                    retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        let name = sample.identity().unwrap_or("");
        if name != identity {
            identity = name.into();
            history = History::default();
        }
        if now.duration_since(last_sample) >= PERIOD {
            // Delayed frames never fill history with repeated stale samples.
            if now.duration_since(last_sample) > Duration::from_millis(120) {
                history.push(None);
            }
            history.push(sample.peak);
            last_sample = now;
        }
        if crate::lcd::editor_open() {
            shown = None;
            colour = None;
            continue;
        }
        if now < write_retry {
            continue;
        }
        let frame = render(&sample, &history);
        let rgb = sample.colour();
        let refresh = last_refresh.elapsed() >= Duration::from_secs(1);
        if colour != Some(rgb) || refresh {
            if crate::daemon::send(&format!("rgb {} {} {}\n", rgb[0], rgb[1], rgb[2])).is_ok() {
                colour = Some(rgb);
            } else {
                write_retry = Instant::now() + Duration::from_secs(1);
                continue;
            }
        }
        if shown != Some(frame) || refresh {
            if crate::daemon::send_lcd(&frame).is_ok() {
                shown = Some(frame);
            } else {
                write_retry = Instant::now() + Duration::from_secs(1);
            }
        }
        if refresh {
            last_refresh = Instant::now();
        }
    }
}

pub fn command(args: &[&str]) -> Result<String, String> {
    if args == ["inputs"] {
        let inputs = inputs()?;
        return Ok(inputs
            .devices
            .iter()
            .map(|d| {
                format!(
                    "{}\t{}{}",
                    d.name,
                    d.label(),
                    if inputs.default.as_deref() == Some(d.name.as_str()) {
                        " [system default]"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let microphone = profile_microphone();
    if let ["preview", path] = args {
        let mut client = pulse::Client::connect(microphone.as_deref())?;
        let mut history = History::default();
        for _ in 0..80 {
            client.tick(true)?;
            history.push(client.peak());
            thread::sleep(PERIOD);
        }
        let (source, sink) = client.devices();
        fs::write(
            Path::new(path),
            render(
                &Sample {
                    source,
                    sink,
                    peak: client.peak(),
                },
                &history,
            ),
        )
        .map_err(|e| e.to_string())?;
        return Ok(format!("wrote System audio preview to {path}"));
    }
    let action = match args { ["status"] => None, [word] => Some(Action::parse(word).ok_or("unknown audio control")?), _ => return Err("usage: g13map audio status|mic-toggle|mic-mute|mic-unmute|mic-up|mic-down|output-toggle|volume-up|volume-down|preview FILE".into()) };
    let mut client = pulse::Client::connect(microphone.as_deref())?;
    if let Some(action) = action {
        client.perform(action)?;
        return Ok(format!("system audio: {}", action.label()));
    }
    let (source, sink) = client.devices();
    let describe = |d: Option<pulse::Device>| {
        d.map(|d| {
            format!(
                "{}: {} {}%",
                d.description,
                if d.muted { "muted" } else { "open" },
                d.percent()
            )
        })
        .unwrap_or("unavailable".into())
    };
    Ok(format!(
        "Microphone {}\nOutput {}",
        describe(source),
        describe(sink)
    ))
}

/// Read-only MPD output collector. A separate owning thread drains peak traffic
/// even while the LCD thread is querying metadata or rasterizing a title.
pub(crate) struct MusicPeaks {
    enabled: std::sync::Arc<std::sync::Mutex<(bool, Option<u32>)>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    peak: std::sync::Arc<std::sync::Mutex<Option<f32>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl MusicPeaks {
    pub fn start() -> Self {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        };
        let enabled = Arc::new(Mutex::new((false, None)));
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(Mutex::new(None));
        let (on, flag, value) = (enabled.clone(), stop.clone(), peak.clone());
        let worker = std::thread::spawn(move || {
            let mut client: Option<pulse::MusicClient> = None;
            let mut process = None;
            let mut retry = Instant::now();
            while !flag.load(Ordering::Relaxed) {
                let (enabled, pid) = *on.lock().unwrap();
                if process != pid {
                    client = None;
                    process = pid;
                    retry = Instant::now();
                }
                if enabled {
                    if client.is_none() && Instant::now() >= retry {
                        client = pulse::MusicClient::connect_for_process(pid).ok();
                        retry = Instant::now() + Duration::from_secs(3);
                    }
                    let next = match &mut client {
                        Some(c) => match c.tick() {
                            Ok(v) => v,
                            Err(_) => {
                                client = None;
                                retry = Instant::now() + Duration::from_secs(3);
                                None
                            }
                        },
                        None => None,
                    };
                    *value.lock().unwrap() = next;
                } else {
                    client = None;
                    *value.lock().unwrap() = None;
                    retry = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Self {
            enabled,
            stop,
            peak,
            worker: Some(worker),
        }
    }
    pub fn enable_process(&self, on: bool, pid: Option<u32>) {
        *self.enabled.lock().unwrap() = (on, pid);
    }
    pub fn sample(&self) -> Option<f32> {
        *self.peak.lock().unwrap()
    }
}
impl Drop for MusicPeaks {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_level_history_and_unavailable_state_do_not_masquerade_as_voice() {
        assert_eq!(Sample::default().colour(), [255, 0, 0]);
        assert_eq!(dbfs(0.0), -60.0);
        assert!((dbfs(0.1) + 20.0).abs() < 0.001);
        let mut h = History::default();
        h.push(Some(0.1));
        assert_eq!(h.at(W - 1), Some(0.1));
        assert_eq!(h.at(0), None);
        let frame = Bitmap(render(&Sample::default(), &h).into());
        assert!(frame.get(W - 1, 31));
        assert!(!frame.get(W - 1, 24));
        h.push(Some(1.0));
        let frame = Bitmap(render(&Sample::default(), &h).into());
        assert!(frame.get(W - 1, 24));
        for _ in 0..W {
            h.push(None);
        }
        assert!(h.values.iter().all(Option::is_none));
    }
    #[test]
    fn audio_bindings_are_exact_and_never_shell_commands() {
        for a in Action::ALL {
            assert_eq!(Action::from_binding(&a.binding()), Some(a));
        }
        for s in [
            ">AUDIO:mic-toggle",
            ">AUDIO:mic-toggle;extra",
            ">AUDIO:unknown;",
            "!mic-toggle",
        ] {
            assert!(Action::from_binding(s).is_none());
        }
    }
}
