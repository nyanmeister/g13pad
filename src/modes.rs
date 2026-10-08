// SPDX-License-Identifier: GPL-3.0-or-later
//! M-keys as profile selectors. M1, M2 and M3 each toggle a bit (sum 1..7), MR clears them,
//! and every sum names a profile in `~/.config/g13map/modes`; sum 0 is the default. The
//! daemon reports the presses through its output FIFO (`bind M1 >M1;`, the `;` because the
//! daemon writes the text with no separator) and `g13map watch` reads them and switches.
//! The owner's design (asked 2026-09-29): additive, up to seven profiles besides the default.
//! `watch` also follows i3 focus changes (`focus`): the window's rule is the base, a lit sum
//! overrides it, MR clears back to it.
use crate::focus::{self, Rules};
use crate::profile::Profile;
use crate::{active_name, daemon, lcd, load, set_active};
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
    seen: (
        String,
        Option<String>,
        Option<SystemTime>,
        Option<SystemTime>,
    ),
}

impl Panel {
    fn state(
        profile: &str,
    ) -> (
        String,
        Option<String>,
        Option<SystemTime>,
        Option<SystemTime>,
    ) {
        let pic = load(profile).ok().and_then(|(p, _)| p.lcd);
        let m = |ext: &str| {
            pic.as_ref()
                .and_then(|n| mtime(&lcd::dir().join(format!("{n}.{ext}"))))
        };
        (profile.to_string(), pic.clone(), m("lpbm"), m("anim"))
    }
    /// Puts the profile's picture up from the start (an animation from its first frame).
    fn show(&mut self, profile: &str) -> Result<(), String> {
        self.seen = Self::state(profile);
        self.player = None; // stops the old frames before the new first one
        self.player = Some(lcd::show(self.seen.1.as_deref(), true)?);
        Ok(())
    }
    /// Shows again when the profile's picture changed, or nothing is up (the daemon was away).
    fn refresh(&mut self, profile: &str) {
        if self.player.is_none() || Self::state(profile) != self.seen {
            if let Err(e) = self.show(profile) {
                eprintln!("LCD: {e}");
                let _ = crate::overlay::notify(&e);
            }
        }
    }
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

/// `g13map watch`: switches profiles on M-key presses (the daemon's output FIFO) and on i3
/// focus changes (its IPC socket), whichever is on. Also restores temporary LCD errors
/// and follows driver reconnects, including profiles with a static LCD.
pub fn watch() -> Result<String, String> {
    let mut modes = Modes::load();
    let mut rules = Rules::load();
    let mut name = active_name();
    let mut bits = 0u8;
    let mut out: Option<Out> = None;
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
    let mut checked = Instant::now();
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
        if editor_checked.elapsed() >= Duration::from_millis(500) {
            editor_checked = Instant::now();
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
                    out = None;
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
            if let Some(o) = &mut out {
                let now = fs::metadata(out_pipe()).ok().map(|m| m.ino());
                if now != o.ino {
                    if let Ok(no) = Out::open() {
                        *o = no;
                        let _ = daemon::send(&format!("{}{}", mod_line(bits, &name), binds()));
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
