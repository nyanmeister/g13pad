// SPDX-License-Identifier: GPL-3.0-or-later
//! M-keys as profile selectors. M1, M2 and M3 each toggle a bit (sum 1..7), MR clears them,
//! and every sum names a profile in `~/.config/g13map/modes`; sum 0 is the default. The
//! daemon reports the presses through its output FIFO (`bind M1 >M1;`, the `;` because the
//! daemon writes the text with no separator) and `g13map watch` reads them and switches.
//! The owner's design (asked 2026-09-29): additive, up to seven profiles besides the default.
//! `watch` also follows window focus changes (`focus`): the window's rule is the base, a lit sum
//! overrides it, MR clears back to it.
use crate::focus::{self, Rules};
use crate::profile::{Display, Profile};
use crate::{active_name, daemon, daemon_base, lcd, load, meter, set_active};
use std::{
    env, fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
};

pub const OUT_PIPE: &str = "/run/g13d/g13-0_out";
pub const KEYS: [&str; 4] = ["M1", "M2", "M3", "MR"];
const O_NONBLOCK: i32 = 0o4000;
/// How long the watcher waits for the session to have a display before giving up on
/// floating the source pointer (a login that never starts X: Wayland, a console).
const POINTER_WAIT: Duration = Duration::from_secs(300);

pub fn path() -> PathBuf {
    crate::config_dir().join("modes")
}

pub fn out_pipe() -> PathBuf {
    env::var_os("G13MAP_OUT_PIPE")
        .map(PathBuf::from)
        .unwrap_or_else(|| OUT_PIPE.into())
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Modes {
    pub on: bool,
    /// Profile per bit sum (index 0..8); None means the default for 0, and for the others
    /// whatever sum 0 names.
    pub profiles: [Option<String>; 8],
}

impl Modes {
    pub fn load() -> Modes {
        Modes::parse(&fs::read_to_string(path()).unwrap_or_default())
    }
    pub fn parse(text: &str) -> Modes {
        let mut m = Modes::default();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match line {
                "on" => m.on = true,
                "off" => m.on = false,
                _ => {
                    let mut it = line.splitn(2, char::is_whitespace);
                    let n: Option<usize> = it.next().and_then(|s| s.parse().ok());
                    let name = it.next().unwrap_or("").trim();
                    if let Some(n) = n.filter(|&n| n < 8) {
                        m.profiles[n] = Some(name.to_string()).filter(|s| !s.is_empty());
                    }
                }
            }
        }
        m
    }
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# g13map: M-Sum mode. M1=1 M2=2 M3=4 toggle and add up; MR clears.\n\
             # `SUM PROFILE` lines; a sum with no line falls back to sum 0 (default).\n",
        );
        out.push_str(if self.on { "on\n" } else { "off\n" });
        for (i, p) in self.profiles.iter().enumerate() {
            if let Some(p) = p {
                out.push_str(&format!("{i} {p}\n"));
            }
        }
        out
    }
    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(crate::config_dir()).map_err(|e| e.to_string())?;
        let p = path();
        let tmp = p.with_extension("new");
        fs::write(&tmp, self.to_text())
            .and_then(|_| fs::rename(&tmp, &p))
            .map_err(|e| format!("{}: {e}", p.display()))
    }
    /// The profile a bit sum selects.
    pub fn profile_for(&self, bits: u8) -> String {
        self.profiles[(bits & 7) as usize]
            .clone()
            .or_else(|| self.profiles[0].clone())
            .unwrap_or_else(|| "default".into())
    }
    /// The lowest bit sum whose profile is `name`: what the LEDs should show for it.
    pub fn bits_for(&self, name: &str) -> Option<u8> {
        (0..8u8).find(|&b| self.profile_for(b) == name)
    }
}

/// Bits after a press of `key`, if it is an M-key.
pub fn step(bits: u8, key: &str) -> Option<u8> {
    match key {
        "M1" => Some(bits ^ 1),
        "M2" => Some(bits ^ 2),
        "M3" => Some(bits ^ 4),
        "MR" => Some(0),
        _ => None,
    }
}

/// Human name for a bit sum: "M1+M3".
pub fn label(bits: u8) -> String {
    if bits == 0 {
        return "none (MR)".into();
    }
    (0..3)
        .filter(|i| bits >> i & 1 == 1)
        .map(|i| format!("M{}", i + 1))
        .collect::<Vec<_>>()
        .join("+")
}

/// The bind lines that route the M-keys to the output FIFO, sent after every profile while
/// modes are on (they override whatever the profile binds to M-keys).
pub fn binds() -> String {
    KEYS.iter().map(|k| format!("bind {k} >{k};\n")).collect()
}

/// What the M-LEDs show in M-Sum mode: the lit sum, or with none lit the profile's own `mod`
/// line, so a profile stays recognisable at a glance until a press takes the LEDs over.
pub fn leds_for(bits: u8, profile: &Profile) -> u8 {
    if bits != 0 {
        bits
    } else {
        profile.leds.unwrap_or(0)
    }
}

/// The `mod` line for the profile called `name` under `bits`.
fn mod_line(bits: u8, name: &str) -> String {
    let shown = load(name).map(|(p, _)| leds_for(bits, &p)).unwrap_or(bits);
    format!("mod {shown}\n")
}

/// Sends profile `to` given the daemon currently has `from`; with `leds` (modes on) also the
/// mode LEDs and the M-key routes; marks it active. Returns the name of its LCD picture,
/// for the caller to show (`lcd::show`: a still once, an animation from a thread).
pub fn switch_to(
    from: Option<&Profile>,
    to_name: &str,
    leds: Option<u8>,
) -> Result<Option<String>, String> {
    let (to, _) = load(to_name)?;
    let baseline = crate::daemon_base();
    crate::application::Plan {
        target: &to,
        previous: from,
        baseline: baseline.as_ref(),
        routing: leds.map_or(
            crate::application::Routing::Profile,
            crate::application::Routing::Sum,
        ),
    }
    .execute()?;
    set_active(to_name)?;
    Ok(to.lcd)
}

/// What the panel shows: the player, and the profile and picture files it was started from,
/// so a picture converted again under the same name is noticed.
#[derive(Default)]
struct Panel {
    player: Option<lcd::Player>,
    /// The health meter while a game feeds it; it owns the panel and the backlight.
    meter: Option<meter::Live>,
    pressure: Option<crate::pressure::Live>,
    mpv: Option<crate::mpv::Live>,
    mpd: Option<crate::mpd::Live>,
    audio: Option<crate::audio::Live>,
    music: Option<crate::music::Worker>,
    /// Feed reads in a row that gave nothing while the meter was on: a feeder rewriting
    /// its file in place is empty for an instant, which must not drop the meter.
    misses: u32,
    seen: PanelState,
}

#[derive(Default, PartialEq)]
struct PanelState {
    profile: String,
    /// The kept picture, and its files' times: a picture converted again under the same
    /// name is noticed.
    picture: Option<String>,
    bitmap: Option<SystemTime>,
    animation: Option<SystemTime>,
    display: Display,
    /// The chosen microphone, while the display is the audio one.
    audio_mic: Option<String>,
}

impl Panel {
    fn state(profile: &str) -> PanelState {
        let selected = load(profile).ok().map(|(p, _)| p);
        let display = selected.as_ref().map(|p| p.display).unwrap_or_default();
        let audio_mic = selected
            .as_ref()
            .filter(|_| display == Display::Audio)
            .and_then(|p| p.audio_mic.clone());
        let pic = selected.and_then(|p| p.lcd);
        let m = |ext: &str| {
            pic.as_ref()
                .and_then(|n| mtime(&lcd::dir().join(format!("{n}.{ext}"))))
        };
        PanelState {
            profile: profile.to_string(),
            picture: pic.clone(),
            bitmap: m("lpbm"),
            animation: m("anim"),
            display,
            audio_mic,
        }
    }
    /// Puts up what the profile chose: its picture from the start (an animation from its
    /// first frame), or its live display. Every other live display is ended first; the
    /// audio and MPD collectors are kept running but told to stop drawing, by design
    /// (their histories survive a profile change). The health meter stays across a switch
    /// between two profiles with the same reader (the switch wrote the backlight, so the
    /// band's colour goes back up over it).
    fn show(&mut self, profile: &str) -> Result<(), String> {
        let was = self.seen.display;
        self.seen = Self::state(profile);
        let want = self.seen.display;
        self.player = None; // stops the old frames before the new first one
        let rest = rest_colour(profile);
        // Leaving the audio display: it painted the backlight, so the profile's comes back.
        if was == Display::Audio && want != Display::Audio {
            if let Some(live) = &self.audio {
                live.display(None, None);
            }
            if !lcd::editor_open() {
                daemon::send(&format!("rgb {} {} {}\n", rest[0], rest[1], rest[2]))?;
            }
        }
        if want != Display::Mpd {
            if let Some(live) = &self.mpd {
                live.display(false);
            }
        }
        if !matches!(want, Display::Health(_)) {
            if let Some(mut live) = self.meter.take() {
                // The meter restores the resting colour as it stops: this profile's,
                // not the one it started under.
                live.rest(rest);
                drop(live);
                eprintln!("health meter: off");
            }
        }
        if want != Display::Mpv && self.mpv.take().is_some() {
            eprintln!("mpv playback: off");
        }
        if want != Display::Pressure && self.pressure.take().is_some() {
            eprintln!("system pressure: off");
        }
        match want {
            Display::Picture => {
                self.player = Some(lcd::show(self.seen.picture.as_deref(), true)?);
            }
            Display::Health(reader) => match &mut self.meter {
                Some(live) if live.reader() == reader => live.rest(rest),
                _ => {
                    self.meter = None; // the old reader goes before the new one binds
                    let state = meter::read().unwrap_or(meter::State::Wait);
                    self.meter = Some(meter::Live::start(state, rest, reader, profile));
                    self.misses = 0;
                    eprintln!("health meter: on ('{profile}', {reader:?})");
                }
            },
            Display::Mpv => {
                if self.mpv.is_none() {
                    self.mpv = Some(crate::mpv::Live::start());
                    eprintln!("mpv playback: on ('{profile}')");
                }
            }
            Display::Pressure => {
                if self.pressure.is_none() {
                    self.pressure = Some(crate::pressure::Live::start());
                    eprintln!("system pressure: on ('{profile}')");
                }
            }
            Display::Audio => {
                self.audio
                    .get_or_insert_with(crate::audio::Live::start)
                    .display(Some(rest), self.seen.audio_mic.clone());
            }
            Display::Mpd => {
                if self.mpd.is_none() {
                    self.mpd = Some(crate::mpd::Live::start());
                    eprintln!("MPD playback: on ('{profile}')");
                }
                self.mpd.as_ref().unwrap().display(true);
            }
        }
        Ok(())
    }
    /// Shows again when the profile's picture changed, or nothing is up (the daemon was away).
    fn refresh(&mut self, profile: &str) {
        let nothing_up = self.player.is_none()
            && self.meter.is_none()
            && self.pressure.is_none()
            && self.mpv.is_none()
            && !matches!(self.seen.display, Display::Mpd | Display::Audio);
        if nothing_up || Self::state(profile) != self.seen {
            if let Err(e) = self.show(profile) {
                eprintln!("LCD: {e}");
                let _ = crate::overlay::notify(&e);
            }
        }
    }
}

/// Empty feed reads in a row (half a second) before the meter shows the wait: a feeder
/// rewriting its file in place is empty for an instant.
const FEED_GRACE: u32 = 5;

/// The feed's state into the meter, while a profile shows one.
fn feed_meter(panel: &mut Panel, state: Option<meter::State>) {
    let Some(live) = &mut panel.meter else { return };
    match state {
        Some(s) => {
            panel.misses = 0;
            live.set(s);
        }
        None => {
            panel.misses = panel.misses.saturating_add(1);
            if panel.misses >= FEED_GRACE {
                live.set(meter::State::Wait);
            }
        }
    }
}

/// The backlight the profile calls for: its own line, else the daemon's startup colour.
fn rest_colour(profile: &str) -> [u8; 3] {
    load(profile)
        .ok()
        .and_then(|(p, _)| p.rgb)
        .or_else(|| daemon_base().and_then(|b| b.rgb))
        .unwrap_or([0, 0, 255])
}

fn open_out() -> Result<fs::File, String> {
    let p = out_pipe();
    fs::File::options()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(&p)
        .map_err(|e| format!("cannot open {}: {e}", p.display()))
}

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    fs::metadata(p).ok()?.modified().ok()
}

/// The profile the state calls for: lit M-keys first (while modes are on), then the focused
/// window's rule (while those are on), then sum 0 (the default).
pub fn target(modes: &Modes, rules: &Rules, bits: u8, focus: Option<&str>) -> String {
    if modes.on && bits != 0 {
        return modes.profile_for(bits);
    }
    if rules.on {
        if let Some(p) = focus.and_then(|c| rules.profile_for(c)) {
            return p.to_string();
        }
    }
    modes.profile_for(0)
}

/// The daemon's output FIFO while modes are on: the file and its inode (the daemon recreates
/// its FIFOs when it restarts).
struct Out {
    f: fs::File,
    ino: Option<u64>,
}

impl Out {
    /// Opens it and drains old presses queued while nobody listened: they are not orders.
    fn open() -> Result<Out, String> {
        let mut f = open_out()?;
        let mut buf = [0u8; 256];
        while matches!(f.read(&mut buf), Ok(n) if n > 0) {}
        let ino = fs::metadata(out_pipe()).ok().map(|m| m.ino());
        Ok(Out { f, ino })
    }
}

/// `g13map watch`: switches profiles on M-key presses (the daemon's output FIFO) and on
/// window focus changes (i3's IPC socket, else the X11 root window), whichever is on. Also
/// restores temporary LCD errors and follows driver reconnects, including profiles with a
/// static LCD.
pub fn watch() -> Result<String, String> {
    let mut modes = Modes::load();
    let mut rules = Rules::load();
    let mut name = active_name();
    let mut bits = 0u8;
    let mut out: Option<Out> = Out::open().ok();
    if modes.on {
        // With window rules on the LEDs are the presses only; alone, they show the profile's sum.
        bits = if rules.on {
            0
        } else {
            modes.bits_for(&name).unwrap_or(0)
        };
        daemon::send(&format!("{}{}", mod_line(bits, &name), binds()))?;
        out = Some(Out::open()?);
    }
    let (tx, rx) = mpsc::channel();
    let mut following = false;
    let mut focus: Option<String> = None;
    let start_following = |following: &mut bool, focus: &mut Option<String>| {
        if !*following {
            *following = true;
            let tx = tx.clone();
            thread::spawn(move || focus::follow(tx));
            *focus = focus::focused_class().filter(|c| c != focus::EDITOR_CLASS);
        }
    };
    if rules.on {
        start_following(&mut following, &mut focus);
    }
    let mut buf = [0u8; 256];
    let mut pending = String::new();
    let mut modes_seen = mtime(&path());
    let mut rules_seen = mtime(&focus::path());
    let mut tuning_seen = meter::Tuning::stamp(&name);
    let mut checked = Instant::now();
    meter::prepare();
    eprintln!(
        "g13map watch: '{name}'; modes {}, window rules {}",
        if modes.on { "on" } else { "off" },
        if rules.on { "on" } else { "off" }
    );
    // Sends what the state calls for, if it is not already on the device.
    let apply = |modes: &Modes,
                 rules: &Rules,
                 bits: u8,
                 focus: Option<&str>,
                 name: &mut String,
                 panel: &mut Panel,
                 why: &str| {
        let t = target(modes, rules, bits, focus);
        let leds = modes.on.then_some(bits);
        let same = t == *name;
        let r = if same {
            match leds {
                Some(b) => daemon::send(&mod_line(b, name)),
                None => Ok(()),
            }
        } else {
            let from = load(name).ok().map(|(p, _)| p);
            switch_to(from.as_ref(), &t, leds).and_then(|_| {
                *name = t.clone();
                panel.show(&t)
            })
        };
        // One journal line per switch, none for a focus change that keeps the profile.
        match r {
            Ok(()) if same && leds.is_none() => {}
            Ok(()) => eprintln!("{why}: '{t}'"),
            Err(e) => {
                eprintln!("{why}: {e}");
                let _ = crate::overlay::notify(&e);
            }
        }
    };
    // The panel: the active profile's picture, an animation playing, unless an editor is
    // open (it shows its own); when the editor closes, what it saved goes up.
    let mut panel = Panel::default();
    let mut editor_was = lcd::editor_open();
    let mut editor_checked = Instant::now();
    let mut meter_checked = Instant::now();
    let mut driver_seen = fs::metadata(daemon::pipe_path())
        .ok()
        .map(|m| (m.dev(), m.ino()));
    let mut health_error: Option<String> = None;
    // The G13's source pointer is floated at login and after a driver reconnect: a few
    // tries, spaced by the 2 s tick, once a display is known. Started by systemd at login
    // there is none yet (see `session`), so the tries wait for one, up to the deadline.
    let mut pointer_checks = 5;
    let mut pointer_why = "login";
    let mut pointer_deadline = Instant::now() + POINTER_WAIT;
    if rules.on {
        apply(
            &modes,
            &rules,
            bits,
            focus.as_deref(),
            &mut name,
            &mut panel,
            "start",
        );
    }
    if !editor_was {
        panel.refresh(&name);
    }
    loop {
        let _ = crate::overlay::tick();
        let mut idle = true;
        if let Some(o) = &mut out {
            match o.f.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    idle = false;
                    pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                    while let Some(i) = pending.find(';') {
                        let tok = pending[..i].trim().to_string();
                        pending.drain(..=i);
                        if let Some(action) = tok
                            .strip_prefix("AUDIO:")
                            .and_then(crate::audio::Action::parse)
                        {
                            panel
                                .audio
                                .get_or_insert_with(crate::audio::Live::start)
                                .action(
                                    action,
                                    load(&active_name()).ok().and_then(|(p, _)| p.audio_mic),
                                );
                            continue;
                        }
                        if let Some(action) = tok
                            .strip_prefix("MPD:")
                            .and_then(crate::music::Action::parse)
                        {
                            panel
                                .music
                                .get_or_insert_with(crate::music::Worker::start)
                                .action(action);
                            continue;
                        }
                        if let Some(key) = crate::run::key_of_token(&tok) {
                            let profile = active_name();
                            let r = load(&profile).and_then(|(p, _)| {
                                let text = p.runs.get(key).ok_or_else(|| {
                                    format!("no `# run {key}` line in profile {profile}")
                                })?;
                                let program = crate::run::Program::parse(text)?;
                                crate::run::start(key, &profile, &program)
                            });
                            match r {
                                Ok(line) => eprintln!("g13map watch: {line}"),
                                Err(e) => eprintln!("g13map watch: run {key}: {e}"),
                            }
                            continue;
                        }
                        if !modes.on {
                            continue;
                        }
                        let Some(b) = step(bits, &tok) else { continue };
                        bits = b;
                        let why = format!("mode {bits} ({})", label(bits));
                        apply(
                            &modes,
                            &rules,
                            bits,
                            focus.as_deref(),
                            &mut name,
                            &mut panel,
                            &why,
                        );
                    }
                    if pending.len() > 64 {
                        pending.clear(); // some other `>` output, not ours
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(format!("read {}: {e}", out_pipe().display())),
            }
        }
        while let Ok(class) = rx.try_recv() {
            if class == focus::EDITOR_CLASS || focus.as_deref() == Some(class.as_str()) {
                continue;
            }
            idle = false;
            focus = Some(class.clone());
            if rules.on {
                apply(
                    &modes,
                    &rules,
                    bits,
                    focus.as_deref(),
                    &mut name,
                    &mut panel,
                    &format!("window '{class}'"),
                );
            }
        }
        if idle {
            thread::sleep(Duration::from_millis(100));
        }
        if meter_checked.elapsed() >= meter::POLL {
            meter_checked = Instant::now();
            if !editor_was {
                feed_meter(&mut panel, meter::read());
            }
        }
        if editor_checked.elapsed() >= Duration::from_millis(500) {
            editor_checked = Instant::now();
            if let Err(e) = crate::demo::recover() {
                eprintln!("demo recovery: {e}");
            }
            let now = lcd::editor_open();
            if editor_was && !now {
                // Whatever the editor showed last, saved or not, the saved picture goes up.
                if let Err(e) = panel.show(&active_name()) {
                    eprintln!("editor closed: {e}");
                    let _ = crate::overlay::notify(&e);
                }
            }
            editor_was = now;
        }
        if checked.elapsed() >= Duration::from_secs(2) {
            checked = Instant::now();
            if !editor_was {
                panel.refresh(&name);
            }
            let tuning_now = meter::Tuning::stamp(&name);
            if tuning_now != tuning_seen {
                tuning_seen = tuning_now;
                if let Some(live) = &mut panel.meter {
                    live.tune(meter::Tuning::load_for(&name));
                    eprintln!("health meter: look re-read");
                }
            }
            let driver_now = fs::metadata(daemon::pipe_path())
                .ok()
                .map(|m| (m.dev(), m.ino()));
            if driver_now.is_some() && driver_now != driver_seen {
                // X11 may discover the new source pointer after the FIFO exists.
                // Retry briefly, targeting only pointer:G13 through the session helper.
                pointer_checks = 5;
                pointer_why = "driver reconnected";
                pointer_deadline = Instant::now() + POINTER_WAIT;
            }
            if pointer_checks > 0 && daemon::up() {
                use crate::session::Detach;
                match crate::session::detach_pointer() {
                    Ok(Detach::NoDisplay) => {
                        if Instant::now() >= pointer_deadline {
                            pointer_checks = 0;
                            eprintln!("{pointer_why}: {}", Detach::NoDisplay);
                        }
                    }
                    Ok(Detach::Absent) => pointer_checks -= 1,
                    Ok(done) => {
                        pointer_checks = 0;
                        eprintln!("{pointer_why}: {done}");
                    }
                    Err(error) => {
                        pointer_checks -= 1;
                        eprintln!("{pointer_why}: {error}");
                    }
                }
            }
            if driver_now.is_some() && driver_now != driver_seen && daemon::up() {
                let result = switch_to(None, &active_name(), modes.on.then_some(bits));
                if let Err(e) = result {
                    eprintln!("driver reconnected: {e}");
                    let _ = crate::overlay::notify(&e);
                } else {
                    panel.player = None;
                    eprintln!("driver reconnected: restored '{}'", active_name());
                }
            }
            driver_seen = driver_now;
            let fault = if !daemon::up() {
                Some("G13 driver unavailable".to_string())
            } else if load(&name)
                .ok()
                .is_some_and(|(p, _)| p.stick == Some(crate::profile::StickMode::Analog))
                && !daemon::analog_active()
            {
                Some("Analog adapter unavailable".to_string())
            } else {
                None
            };
            if fault != health_error {
                if let Some(e) = &fault {
                    eprintln!("G13: {e}");
                    let _ = crate::overlay::notify(e);
                }
                health_error = fault;
            }
            let seen = (mtime(&path()), mtime(&focus::path()));
            if seen != (modes_seen, rules_seen) {
                (modes_seen, rules_seen) = seen;
                let was = (modes.on, rules.on);
                modes = Modes::load();
                rules = Rules::load();
                if modes.on && !was.0 {
                    bits = if rules.on {
                        0
                    } else {
                        modes.bits_for(&name).unwrap_or(0)
                    };
                    let _ = daemon::send(&format!("{}{}", mod_line(bits, &name), binds()));
                    out = Some(Out::open()?);
                } else if !modes.on && was.0 {
                    // Audio and music actions use this FIFO even when M-Sum is disabled.
                    bits = 0;
                }
                if rules.on {
                    start_following(&mut following, &mut focus);
                }
                if modes.on || rules.on {
                    apply(
                        &modes,
                        &rules,
                        bits,
                        focus.as_deref(),
                        &mut name,
                        &mut panel,
                        "files changed",
                    );
                }
            }
            // The editor may have switched profiles by hand: follow it.
            let active = active_name();
            if active != name {
                name = active;
                if modes.on && !rules.on {
                    bits = modes.bits_for(&name).unwrap_or(bits);
                    let _ = daemon::send(&mod_line(bits, &name));
                }
            }
            if out.is_none() {
                out = Out::open().ok();
            }
            if let Some(o) = &mut out {
                let now = fs::metadata(out_pipe()).ok().map(|m| m.ino());
                if now != o.ino {
                    if let Ok(no) = Out::open() {
                        *o = no;
                        if modes.on {
                            let _ = daemon::send(&format!("{}{}", mod_line(bits, &name), binds()));
                        }
                        eprintln!("g13map watch: reopened {}", out_pipe().display());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every display to every other (the audio one excepted: its collector opens the
    /// machine's sound server), with the rule checked after each step: exactly the chosen
    /// display is up, the picture player only for the picture, the meter with the chosen
    /// reader, and the MPD collector kept alive once started (its history survives).
    #[test]
    #[cfg(feature = "editor")]
    fn every_display_handoff_leaves_exactly_the_chosen_one_up() {
        let _sandbox = crate::test_support::Sandbox::new("display-matrix");
        let _pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("still", &[61]);
        let displays: Vec<Display> = Display::ALL
            .into_iter()
            .filter(|d| *d != Display::Audio)
            .collect();
        for (i, display) in displays.iter().enumerate() {
            let mut p = Profile::parse("# lcd still\n").0;
            p.display = *display;
            crate::save(&format!("d{i}"), &p).unwrap();
        }
        let check = |panel: &Panel, want: Display| {
            assert_eq!(panel.seen.display, want);
            assert_eq!(panel.player.is_some(), want == Display::Picture, "{want:?}");
            assert_eq!(
                panel.pressure.is_some(),
                want == Display::Pressure,
                "{want:?}"
            );
            assert_eq!(panel.mpv.is_some(), want == Display::Mpv, "{want:?}");
            match want {
                Display::Health(reader) => {
                    assert_eq!(panel.meter.as_ref().map(|m| m.reader()), Some(reader))
                }
                _ => assert!(panel.meter.is_none(), "{want:?}"),
            }
            if want == Display::Mpd {
                assert!(panel.mpd.is_some());
            }
        };
        let mut panel = Panel::default();
        let mut started_mpd = false;
        for (i, from) in displays.iter().enumerate() {
            for (j, to) in displays.iter().enumerate() {
                panel.show(&format!("d{i}")).unwrap();
                check(&panel, *from);
                panel.show(&format!("d{j}")).unwrap();
                check(&panel, *to);
                started_mpd |= *to == Display::Mpd;
                assert_eq!(panel.mpd.is_some(), started_mpd, "{from:?} -> {to:?}");
                // Showing the same thing again changes nothing.
                panel.refresh(&format!("d{j}"));
                check(&panel, *to);
            }
        }
    }

    #[test]
    #[cfg(feature = "editor")]
    fn pressure_health_and_picture_handoffs_stop_the_previous_display() {
        let _sandbox = crate::test_support::Sandbox::new("pressure-handoff");
        let pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("still", &[61]);
        let (p, _) = Profile::parse("# lcd still\n# pressure on\n");
        crate::save("pressure", &p).unwrap();
        let (p, _) = Profile::parse("# lcd still\n# health feed\n");
        crate::save("health", &p).unwrap();
        let (p, _) = Profile::parse("# lcd still\n# mpv on\n");
        crate::save("mpv", &p).unwrap();
        let (p, _) = Profile::parse("# lcd still\n# mpd on\n");
        crate::save("mpd", &p).unwrap();
        let mut panel = Panel::default();
        panel.show("pressure").unwrap();
        assert!(panel.pressure.is_some() && panel.meter.is_none() && panel.player.is_none());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.refresh("pressure");
        pipe.expect_quiet();
        panel.show("health").unwrap();
        assert!(panel.pressure.is_none() && panel.meter.is_some() && panel.player.is_none());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("pressure").unwrap();
        assert!(panel.pressure.is_some() && panel.meter.is_none());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("mpv").unwrap();
        assert!(panel.mpv.is_some() && panel.pressure.is_none() && panel.meter.is_none());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("health").unwrap();
        assert!(panel.mpv.is_none() && panel.pressure.is_none() && panel.meter.is_some());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("mpv").unwrap();
        assert!(panel.mpv.is_some() && panel.pressure.is_none() && panel.meter.is_none());
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("mpd").unwrap();
        assert!(
            panel.mpd.is_some()
                && panel.mpv.is_none()
                && panel.pressure.is_none()
                && panel.meter.is_none()
        );
        assert_eq!(pipe.read().len(), lcd::BYTES);
        panel.show("still").unwrap();
        assert!(panel.mpd.is_some() && panel.seen.display != Display::Mpd);
        assert!(
            panel.mpv.is_none()
                && panel.pressure.is_none()
                && panel.meter.is_none()
                && panel.player.is_some()
        );
        // A frame already in flight can precede the new picture, never follow it.
        loop {
            if pipe.read() == vec![61; lcd::BYTES] {
                break;
            }
        }
        pipe.expect_quiet();
    }
    #[test]
    fn automatic_profile_switch_applies_stick_before_top_binding() {
        let _sandbox = crate::test_support::Sandbox::new("watch-stick");
        let pipe = crate::test_support::PanelPipe::new();
        let keys = Profile::parse("# stick keys\nbind TOP KEY_C\n").0;
        let analog = Profile::parse("# stick analog\nbind TOP KEY_D\nbind G1 KEY_A\n").0;
        crate::save("keys", &keys).unwrap();
        crate::save("analog", &analog).unwrap();
        crate::set_active("analog").unwrap();
        switch_to(Some(&analog), "keys", None).unwrap();
        assert!(!daemon::analog_active());
        let commands = String::from_utf8(pipe.read()).unwrap();
        assert!(commands.contains("bind TOP KEY_C\n"));
        assert_eq!(crate::active_name(), "keys");
        switch_to(Some(&keys), "analog", None).unwrap();
        assert!(daemon::analog_active());
        assert!(!String::from_utf8(pipe.read()).unwrap().contains("bind TOP"));
        drop(pipe);
        assert!(switch_to(Some(&analog), "keys", None).is_err());
        assert_eq!(crate::active_name(), "analog");
        assert!(daemon::analog_active());
    }
    #[test]
    fn file_round_trip_and_lookup() {
        let m = Modes::parse("# c\non\n0 default\n1 drg\n5 drg-alt\n9 bogus\n3\n");
        assert!(m.on);
        assert_eq!(m.profiles[1].as_deref(), Some("drg"));
        assert_eq!(m.profiles[3], None);
        assert_eq!(m.profile_for(3), "default"); // unassigned: sum 0's
        assert_eq!(m.profile_for(5), "drg-alt");
        assert_eq!(m.bits_for("drg"), Some(1));
        assert_eq!(m.bits_for("nope"), None);
        assert_eq!(Modes::parse(&m.to_text()), m);
        assert!(!Modes::parse("").on);
        assert_eq!(Modes::parse("").profile_for(7), "default");
    }
    #[test]
    fn target_layers_modes_over_window_rules() {
        let m = Modes::parse("on\n1 drg\n");
        let r = Rules::parse("on\nfirefox\tbrowsing\n");
        let off = Rules::parse("off\nfirefox\tbrowsing\n");
        assert_eq!(target(&m, &r, 0, Some("firefox")), "browsing");
        assert_eq!(target(&m, &r, 1, Some("firefox")), "drg"); // a lit sum wins
        assert_eq!(target(&m, &r, 0, Some("kitty")), "default"); // no rule: sum 0
        assert_eq!(target(&m, &r, 0, None), "default");
        assert_eq!(target(&m, &off, 0, Some("firefox")), "default"); // rules off
        let m_off = Modes::parse("off\n1 drg\n0 base\n");
        assert_eq!(target(&m_off, &r, 1, Some("firefox")), "browsing"); // modes off: bits ignored
        assert_eq!(target(&m_off, &r, 0, Some("kitty")), "base");
    }

    #[test]
    fn leds_show_the_sum_or_the_profile() {
        let (p, _) = Profile::parse("mod 5\nbind G1 KEY_A\n");
        assert_eq!(leds_for(0, &p), 5);
        assert_eq!(leds_for(2, &p), 2);
        let (bare, _) = Profile::parse("bind G1 KEY_A\n");
        assert_eq!(leds_for(0, &bare), 0);
    }

    #[test]
    fn presses_add_up_and_mr_clears() {
        let mut b = 0;
        b = step(b, "M1").unwrap();
        b = step(b, "M3").unwrap();
        assert_eq!((b, label(b).as_str()), (5, "M1+M3"));
        b = step(b, "M1").unwrap();
        assert_eq!(b, 4);
        assert_eq!(step(b, "MR"), Some(0));
        assert_eq!(step(b, "G1"), None);
        assert_eq!(binds().lines().count(), 4);
        assert!(binds().contains("bind MR >MR;"));
    }
}
