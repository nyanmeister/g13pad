// SPDX-License-Identifier: GPL-3.0-or-later
//! A profile is a g13d bind file: `bind KEY ACTION`, `rgb R G B`, `mod N`, other daemon
//! commands kept verbatim. No second format: what the GUI saves is what the daemon reads.
use std::collections::BTreeMap;

/// The bindable controls in diagram order (also the order bindings are written).
pub const CONTROLS: &[&str] = &[
    "G1",
    "G2",
    "G3",
    "G4",
    "G5",
    "G6",
    "G7",
    "G8",
    "G9",
    "G10",
    "G11",
    "G12",
    "G13",
    "G14",
    "G15",
    "G16",
    "G17",
    "G18",
    "G19",
    "G20",
    "G21",
    "G22",
    "M1",
    "M2",
    "M3",
    "MR",
    "BD",
    "L1",
    "L2",
    "L3",
    "L4",
    "LEFT",
    "DOWN",
    "TOP",
    "STICK_UP",
    "STICK_DOWN",
    "STICK_LEFT",
    "STICK_RIGHT",
    "STICK_PAGEUP",
    "STICK_PAGEDOWN",
];
/// The custom driver accepts KEY_RESERVED as a no-op, so this clears a live binding.
/// Stock 1e80eda rejects this name: use the tools/g13d-1e80eda-unbind.patch too.
pub const UNBOUND: &str = "KEY_RESERVED";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickMode {
    Analog,
    Keys,
}

impl StickMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Analog => "analog",
            Self::Keys => "keys",
        }
    }
}

/// What the LCD shows while the profile is active: its kept picture, or one live display.
/// One choice, so the profile cannot ask for two at once (until 2026-10-12 this was five
/// fields clearing each other). The picture (`lcd`) stays kept under any live display, for
/// when the choice goes back to it. On disk each live display is its own comment line
/// (`# health feed`, `# mpv on`, `# pressure on`, `# audio on`, `# mpd on`), unchanged
/// from before, so files written either way read the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Display {
    /// The kept picture, or the daemon's logo without one.
    #[default]
    Picture,
    /// The health meter, fed by that reader (asked 2026-10-08).
    Health(crate::meter::Reader),
    /// mpv playback: elapsed time and progress of the running player.
    Mpv,
    /// The machine's CPU, memory and I/O pressure.
    Pressure,
    /// Microphone and output state with a level history; the mic is `audio_mic`.
    Audio,
    /// MPD: what plays, its history, output levels.
    Mpd,
}

impl Display {
    /// Every choice, in the order the editor lists them.
    pub const ALL: [Self; 10] = {
        use crate::meter::Reader;
        [
            Self::Picture,
            Self::Health(Reader::Feed),
            Self::Health(Reader::Cs2),
            Self::Health(Reader::Log),
            Self::Health(Reader::RetroArch),
            Self::Health(Reader::Dolphin),
            Self::Mpv,
            Self::Mpd,
            Self::Audio,
            Self::Pressure,
        ]
    };
    /// Short name, as the editor's list and the status line show it.
    pub fn label(self) -> &'static str {
        use crate::meter::Reader;
        match self {
            Self::Picture => "Picture",
            Self::Health(Reader::Feed) => "Game health (mod or script)",
            Self::Health(Reader::Cs2) => "Counter-Strike 2 health",
            Self::Health(Reader::Log) => "Doom health (console log)",
            Self::Health(Reader::RetroArch) => "RetroArch achievements",
            Self::Health(Reader::Dolphin) => "Dolphin achievements",
            Self::Mpv => "mpv: what is playing",
            Self::Mpd => "Music (MPD): what is playing",
            Self::Audio => "Microphone and volume",
            Self::Pressure => "Computer load",
        }
    }
    /// One sentence for a layman, under the label.
    pub fn about(self) -> &'static str {
        use crate::meter::Reader;
        match self {
            Self::Picture => "The picture or animation kept with this profile.",
            Self::Health(Reader::Feed) => {
                "Health and shield from a game mod or script that feeds g13map (Deep Rock, Factorio, ULTRAKILL, Terraria, Dwarf Fortress...)."
            }
            Self::Health(Reader::Cs2) => "Health, armour and helmet from the game's own Game State Integration.",
            Self::Health(Reader::Log) => "Health from a Doom engine's console log (Zandronum, GZDoom with the contrib script).",
            Self::Health(Reader::RetroArch) => "Achievement progress and unlock cards from RetroArch's local log.",
            Self::Health(Reader::Dolphin) => "Achievement progress and unlock cards from Dolphin's local log.",
            Self::Mpv => "Elapsed time and a progress bar for the video or track mpv is playing.",
            Self::Mpd => "Artist and title, play history and output levels from the music daemon.",
            Self::Audio => "Whether the microphone is live or muted, its level, and the output volume.",
            Self::Pressure => "How busy the CPU, memory and disks are.",
        }
    }
    /// The comment line that selects this on disk, without the `# `; none for the picture.
    pub fn line(self) -> Option<String> {
        Some(match self {
            Self::Picture => return None,
            Self::Health(reader) => format!("health {}", reader.key()),
            Self::Mpv => "mpv on".into(),
            Self::Pressure => "pressure on".into(),
            Self::Audio => "audio on".into(),
            Self::Mpd => "mpd on".into(),
        })
    }
    /// Reads a `# WORD VALUE` comment: `Some(Ok(display))` to select, `Some(Ok(Picture))`
    /// for an `off`, `Some(Err(why))` for a bad value, `None` for an unrelated comment.
    fn parse(word: &str, value: &str) -> Option<Result<Self, String>> {
        let on_off = |on: Self, what: &str| match value {
            "on" => Ok(on),
            "off" => Ok(Self::Picture),
            _ => Err(format!("ignored invalid {what}: {value}")),
        };
        Some(match word {
            "health" => match value {
                "off" => Ok(Self::Picture),
                _ => crate::meter::Reader::parse(value)
                    .map(Self::Health)
                    .ok_or_else(|| format!("ignored invalid health mode: {value}")),
            },
            "mpv" => on_off(Self::Mpv, "mpv display"),
            "pressure" => on_off(Self::Pressure, "System pressure mode"),
            "audio" => on_off(Self::Audio, "System audio mode"),
            "mpd" => on_off(Self::Mpd, "MPD playback mode"),
            _ => return None,
        })
    }
    /// The word the `off` of this line clears: `# mpv off` leaves another display alone.
    fn word(self) -> &'static str {
        match self {
            Self::Picture => "",
            Self::Health(_) => "health",
            Self::Mpv => "mpv",
            Self::Pressure => "pressure",
            Self::Audio => "audio",
            Self::Mpd => "mpd",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Profile {
    /// control name -> action string in the daemon's grammar (absent = unbound).
    pub binds: BTreeMap<String, String>,
    pub rgb: Option<[u8; 3]>,
    /// M-key LEDs, bit sum: 1 M1, 2 M2, 4 M3, 8 MR.
    pub leds: Option<u8>,
    /// Name of the LCD image in the history (`# lcd NAME`: a comment, so the daemon never
    /// sees it; the frame itself goes through `daemon::send_lcd`).
    pub lcd: Option<String>,
    /// What the LCD shows: the picture above, or a live display over it.
    pub display: Display,
    /// Stable PulseAudio source name for `Display::Audio` and the mic bindings; None
    /// follows the desktop default.
    pub audio_mic: Option<String>,
    /// None preserves the current mode. A comment so the service remains its only owner.
    pub stick: Option<StickMode>,
    pub gamepad: Option<crate::gamepad::Mapping>,
    /// `# run KEY [WHEN] PROGRAM [ARGS...]`: the program `bind KEY >RUN:KEY;` (or, with a
    /// release half, `>RUN:KEY; RELEASE:KEY;`) starts, and when. A comment: the daemon
    /// never runs anything, the watcher does, from the user's own file.
    pub runs: BTreeMap<String, crate::run::Run>,
    /// Other command lines, kept verbatim (stickzone bounds, font, ...).
    pub extra: Vec<String>,
}

impl Profile {
    /// Whether `g13map watch` has to run for this profile to work as saved: a live
    /// display, or a binding the watcher carries out (audio and music controls).
    pub fn needs_watcher(&self) -> bool {
        self.display != Display::Picture
            || self.binds.values().any(|a| {
                crate::audio::Action::from_binding(a).is_some()
                    || crate::music::Action::from_binding(a).is_some()
                    || crate::run::key_of_binding(a).is_some()
            })
    }
    /// The health meter's reader when the profile shows the meter.
    pub fn health(&self) -> Option<crate::meter::Reader> {
        match self.display {
            Display::Health(reader) => Some(reader),
            _ => None,
        }
    }
    /// Parses bind-file text. `stickmode` lines are dropped: the analog adapter service owns
    /// the stick mode and a profile must never flip it. Returns the notes for the caller.
    pub fn parse(text: &str) -> (Profile, Vec<String>) {
        let mut p = Profile::default();
        let mut notes = vec![];
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix('#') {
                if let Some(value) = comment.trim().strip_prefix("audio-mic ") {
                    match serde_json::from_str::<String>(value) {
                        Ok(name) if !name.is_empty() && !name.contains('\0') => {
                            p.audio_mic = Some(name)
                        }
                        _ => notes.push(format!("ignored invalid microphone identity: {line}")),
                    }
                }
                // A live display line: the last one in the file wins; an `off` clears
                // only its own display, so `# mpv off` leaves `# mpd on` alone.
                let (word, value) = comment
                    .trim()
                    .split_once(char::is_whitespace)
                    .unwrap_or((comment.trim(), ""));
                match Display::parse(word, value.trim()) {
                    Some(Ok(Display::Picture)) => {
                        if p.display.word() == word {
                            p.display = Display::Picture;
                        }
                    }
                    Some(Ok(display)) => {
                        if p.display != Display::Picture && p.display != display {
                            notes
                                .push(format!("{} replaces another live display", display.label()));
                        }
                        p.display = display;
                    }
                    Some(Err(why)) => notes.push(why),
                    None => {}
                }
                if let Some(mapping) = comment.trim().strip_prefix("gamepad ") {
                    match crate::gamepad::Mapping::from_compact(mapping) {
                        Ok(mapping) => p.gamepad = Some(mapping),
                        Err(e) => notes.push(format!("ignored invalid gamepad mapping: {e}")),
                    }
                }
                if let Some(mode) = comment.trim().strip_prefix("stick ") {
                    p.stick = match mode.trim() {
                        "analog" => Some(StickMode::Analog),
                        "keys" => Some(StickMode::Keys),
                        "current" => None,
                        _ => {
                            notes.push(format!("ignored invalid stick preference: {line}"));
                            p.stick
                        }
                    };
                }
                if let Some(rest) = comment.trim().strip_prefix("run ") {
                    let mut it = rest.trim().splitn(2, char::is_whitespace);
                    let key = it.next().unwrap_or("");
                    let program = it.next().unwrap_or("").trim();
                    match crate::run::Run::parse(program) {
                        Ok(run) if !key.is_empty() => {
                            p.runs.insert(key.to_string(), run);
                        }
                        Ok(_) => notes.push(format!("ignored malformed line: {line}")),
                        Err(why) => notes.push(format!("ignored run line `{line}`: {why}")),
                    }
                }
                if let Some(name) = comment.trim().strip_prefix("lcd ") {
                    p.lcd = Some(name.trim().to_string()).filter(|n| !n.is_empty());
                }
                continue;
            }
            let mut it = line.splitn(2, char::is_whitespace);
            let cmd = it.next().unwrap_or("");
            let rest = it.next().unwrap_or("").trim();
            match cmd {
                "bind" => {
                    let mut it = rest.splitn(2, char::is_whitespace);
                    let key = it.next().unwrap_or("").to_string();
                    let action = it.next().unwrap_or("").trim().to_string();
                    if key.is_empty() || action.is_empty() {
                        notes.push(format!("ignored malformed line: {line}"));
                    } else if action == UNBOUND {
                        p.binds.remove(&key);
                    } else {
                        p.binds.insert(key, action);
                    }
                }
                "rgb" => {
                    let v: Vec<u8> = rest
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    if v.len() == 3 {
                        p.rgb = Some([v[0], v[1], v[2]]);
                    } else {
                        notes.push(format!("ignored malformed line: {line}"));
                    }
                }
                "mod" => match rest.parse::<u8>() {
                    Ok(n) if n < 16 => p.leds = Some(n),
                    _ => notes.push(format!("ignored malformed line: {line}")),
                },
                "stickmode" => notes.push(format!(
                    "dropped `{line}`: the stick mode belongs to g13-analog.service"
                )),
                _ => p.extra.push(line.to_string()),
            }
        }
        // A program rides two lines, the bind and the `# run`; one without the other is a
        // repair, not a guess.
        for (key, action) in &p.binds {
            if let Some(k) = crate::run::key_of_binding(action) {
                if !p.runs.contains_key(k) {
                    notes.push(format!(
                        "bind {key} {action} has no `# run {k} PROGRAM` line naming the program"
                    ));
                }
            }
        }
        for (key, run) in &p.runs {
            let bound = p.binds.get(key).map(String::as_str).unwrap_or("");
            if crate::run::key_of_binding(bound) != Some(key) {
                notes.push(format!(
                    "`# run {key}` names a program but {key} is not bound `>RUN:{key};`"
                ));
            } else if run.when.needs_release() && !crate::run::binding_reports_release(bound) {
                notes.push(format!(
                    "`# run {key} {}` needs the release too: bind {key} `{}`",
                    run.when.word(),
                    crate::run::binding(key, run.when)
                ));
            }
        }
        // Before `# health`, the meter was a picture name (`# lcd health cs2`): read as
        // health mode with no picture of its own, written back the new way.
        if let Some(reader) = crate::meter::selects(p.lcd.as_deref()) {
            if p.display == Display::Picture {
                p.display = Display::Health(reader);
            }
            p.lcd = None;
        }
        (p, notes)
    }

    /// The file text: a header, then the commands. Comments are on their own lines because
    /// g13d has no trailing-comment syntax.
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# g13d bind file written by g13map. Edit by hand if you like; g13map re-reads it.\n\
             # Keys with no `bind` line are unbound. g13-analog.service owns stick mode.\n",
        );
        if let Some(n) = &self.lcd {
            out.push_str(&format!("# lcd {n}\n"));
        }
        if let Some(line) = self.display.line() {
            out.push_str(&format!("# {line}\n"));
        }
        if let Some(name) = &self.audio_mic {
            out.push_str(&format!(
                "# audio-mic {}\n",
                serde_json::to_string(name).unwrap()
            ));
        }
        if let Some(mode) = self.stick {
            out.push_str(&format!("# stick {}\n", mode.label()));
        }
        if let Some(mapping) = self.gamepad {
            out.push_str(&format!("# gamepad {}\n", mapping.compact()));
        }
        for (key, run) in &self.runs {
            out.push_str(&format!("# run {key} {}\n", run.line()));
        }
        out.push_str(&self.commands(None, false));
        out
    }

    /// Command lines to send to the daemon. With `base` (the daemon's startup config), keys
    /// bound there but not here are sent as KEY_RESERVED so the daemon matches this profile.
    /// `analog_active` skips TOP, which the adapter service binds to the stick click.
    pub fn commands(&self, base: Option<&Profile>, analog_active: bool) -> String {
        self.commands_from(None, base, analog_active)
    }

    /// Reads the target alongside the previous live bindings. Explicit unbinds are
    /// commands in this view, never synthetic bindings inserted into a saved profile.
    pub fn commands_from(
        &self,
        previous: Option<&Profile>,
        base: Option<&Profile>,
        analog_active: bool,
    ) -> String {
        let mut out = String::new();
        if let Some([r, g, b]) = self.rgb {
            out.push_str(&format!("rgb {r} {g} {b}\n"));
        }
        if let Some(n) = self.leds {
            out.push_str(&format!("mod {n}\n"));
        }
        for &key in CONTROLS {
            if analog_active && key == "TOP" {
                continue;
            }
            let was_bound = previous.is_some_and(|p| p.binds.contains_key(key))
                || base.is_some_and(|p| p.binds.contains_key(key));
            match (self.binds.get(key), was_bound) {
                (Some(a), _) => out.push_str(&format!("bind {key} {a}\n")),
                (None, _) if key == "TOP" && !analog_active => {
                    out.push_str(&format!("bind TOP {UNBOUND}\n"))
                }
                (None, true) => out.push_str(&format!("bind {key} {UNBOUND}\n")),
                (None, false) => {}
            }
        }
        // Imported/custom controls retain sorted ordering, including explicit clears.
        let other_keys: std::collections::BTreeSet<&String> = self
            .binds
            .keys()
            .chain(previous.into_iter().flat_map(|p| p.binds.keys()))
            .filter(|key| !CONTROLS.contains(&key.as_str()))
            .collect();
        for key in other_keys {
            let a = self.binds.get(key).map_or(UNBOUND, String::as_str);
            out.push_str(&format!("bind {key} {a}\n"));
        }
        for line in &self.extra {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// A key action split for the editor: modifier names and the main key, when the action is a
/// plain chord like `KEY_LEFTCTRL+KEY_X`. Anything else is raw: release actions, mouse
/// buttons (`MLEFT`), `>`, `!`, and names the daemon's uinput device cannot emit.
pub fn chord(action: &str) -> Option<(Vec<String>, Option<String>)> {
    if action.is_empty() || action.contains(' ') {
        return None;
    }
    let mut mods = vec![];
    let mut main = None;
    for part in action.split('+') {
        let name = part.strip_prefix("KEY_")?;
        if MODIFIERS.iter().any(|(m, _)| *m == name) {
            mods.push(name.to_string());
        } else if !crate::keys::KEYS.iter().any(|&(_, k)| k == name)
            || main.replace(name.to_string()).is_some()
        {
            return None; // unknown key, or two main keys: not a chord the editor models
        }
    }
    Some((mods, main))
}

/// Modifier key names and their labels, in the order chords are written.
pub const MODIFIERS: &[(&str, &str)] = &[
    ("LEFTCTRL", "Ctrl"),
    ("LEFTSHIFT", "Shift"),
    ("LEFTALT", "Alt"),
    ("LEFTMETA", "Super"),
    ("RIGHTCTRL", "RCtrl"),
    ("RIGHTSHIFT", "RShift"),
    ("RIGHTALT", "AltGr"),
];

pub fn chord_action(mods: &[String], main: Option<&str>) -> String {
    let mut parts: Vec<String> = MODIFIERS
        .iter()
        .filter(|(m, _)| mods.iter().any(|x| x == m))
        .map(|(m, _)| format!("KEY_{m}"))
        .collect();
    if let Some(k) = main {
        parts.push(format!("KEY_{k}"));
    }
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microphone_identity_round_trips_without_command_injection_or_mode_loss() {
        let mut p = Profile {
            display: Display::Audio,
            audio_mic: Some("USB \"Mic\" 日本語\nsource".into()),
            ..Profile::default()
        };
        let text = p.to_text();
        assert_eq!(
            text.lines()
                .filter(|s| s.starts_with("# audio-mic "))
                .count(),
            1
        );
        assert_eq!(Profile::parse(&text).0, p);
        // The mic is kept under any display: the mic bindings use it too.
        p.display = Display::Pressure;
        assert_eq!(Profile::parse(&p.to_text()).0.audio_mic, p.audio_mic);
        assert!(Profile::parse("# audio on").0.audio_mic.is_none());
        for bad in [
            "# audio-mic not-json",
            "# audio-mic \"\"",
            "# audio-mic \"\\u0000\"",
        ] {
            assert!(!Profile::parse(bad).1.is_empty());
        }
    }
    /// Every display writes one line of its own and reads back; the picture, mic and
    /// bindings underneath stay. The words are the ones 0.2.85 wrote, so files from
    /// either side of the `Display` change read the same.
    #[test]
    fn every_display_round_trips_and_keeps_the_rest() {
        const WORDS: [&str; 5] = [
            "# health ",
            "# mpv on",
            "# pressure on",
            "# audio on",
            "# mpd on",
        ];
        let mut p = Profile::parse("# lcd kept\nbind G1 KEY_A\n# audio-mic \"usb.mic\"\n").0;
        assert_eq!(p.display, Display::Picture);
        assert!(!p.needs_watcher());
        for display in Display::ALL {
            p.display = display;
            let text = p.to_text();
            let lines = text
                .lines()
                .filter(|l| WORDS.iter().any(|w| l.starts_with(w)))
                .count();
            assert_eq!(lines, (display != Display::Picture) as usize, "{text}");
            let (again, notes) = Profile::parse(&text);
            assert!(notes.is_empty(), "{notes:?}");
            assert_eq!(again, p);
            assert_eq!(p.lcd.as_deref(), Some("kept"));
            assert_eq!(p.audio_mic.as_deref(), Some("usb.mic"));
            assert_eq!(p.binds["G1"], "KEY_A");
            assert_eq!(p.needs_watcher(), display != Display::Picture);
            assert_eq!(p.health().is_some(), matches!(display, Display::Health(_)));
        }
        assert_eq!(Display::ALL.len(), 10);
        assert_eq!(
            Profile::parse("# health cs2").0.display.line().as_deref(),
            Some("health cs2")
        );
    }
    /// Expected values worked out by hand from the rule, not from running the parser: the
    /// last live-display line wins, a replaced one is noted, `off` clears only its own.
    #[test]
    fn last_live_display_line_wins_and_off_clears_only_its_own() {
        use crate::meter::Reader;
        for (text, display, notes) in [
            ("# health feed\n# pressure on", Display::Pressure, 1),
            (
                "# pressure on\n# health feed",
                Display::Health(Reader::Feed),
                1,
            ),
            ("# mpv on\n# pressure on", Display::Pressure, 1),
            ("# audio on\n# pressure on", Display::Pressure, 1),
            ("# pressure on\n# audio on", Display::Audio, 1),
            ("# mpd on\n# mpv on", Display::Mpv, 1),
            ("# mpv on\n# mpd on", Display::Mpd, 1),
            ("# health feed\n# pressure on\n# mpv on", Display::Mpv, 2),
            ("# mpd on\n# mpv off", Display::Mpd, 0),
            ("# mpd on\n# mpd off", Display::Picture, 0),
            ("# health cs2\n# health off", Display::Picture, 0),
            (
                "# health cs2\n# health cs2",
                Display::Health(Reader::Cs2),
                0,
            ),
            (
                "# health cs2\n# health log",
                Display::Health(Reader::Log),
                1,
            ),
            ("# pressure nonsense", Display::Picture, 1),
            ("# health yes", Display::Picture, 1),
            ("# audio nonsense", Display::Picture, 1),
            ("# mpd nonsense", Display::Picture, 1),
            ("# mpv maybe", Display::Picture, 1),
            ("# mpvish on", Display::Picture, 0),
            ("# health", Display::Picture, 1),
        ] {
            let (p, got) = Profile::parse(text);
            assert_eq!(p.display, display, "{text:?}");
            assert_eq!(got.len(), notes, "{text:?}: {got:?}");
        }
    }
    #[test]
    fn controller_metadata_round_trips_without_becoming_a_driver_command() {
        let (p, notes) = Profile::parse("# gamepad right r3 1 1 0\nbind G1 KEY_A\n");
        assert!(notes.is_empty());
        assert_eq!(Profile::parse(&p.to_text()).0, p);
        assert!(!p.commands(None, true).contains("gamepad"));
        let (bad, notes) =
            Profile::parse("# gamepad right r3 1 1 0\n# gamepad left injected 0 0 1\n");
        assert_eq!(bad.gamepad, p.gamepad);
        assert_eq!(notes.len(), 1);
    }
    /// A program rides two lines: the bind the daemon sees and the `# run` comment naming
    /// the program. Both read back; either alone is a note (a repair, not a guess).
    #[test]
    fn programs_round_trip_and_half_a_binding_is_a_note() {
        let text = "# run G5 ~/bin/lights.sh on\nbind G5 >RUN:G5;\nbind G6 KEY_A\n";
        let (p, notes) = Profile::parse(text);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(p.runs["G5"].line(), "~/bin/lights.sh on");
        assert_eq!(p.runs["G5"].when, crate::run::When::Pressed);
        assert_eq!(p.binds["G5"], ">RUN:G5;");
        assert!(p.needs_watcher());
        let again = p.to_text();
        assert_eq!(again.lines().filter(|l| l.starts_with("# run ")).count(), 1);
        assert_eq!(Profile::parse(&again).0, p);
        // The daemon gets the bind and never the comment.
        let sent = p.commands(None, false);
        assert!(sent.contains("bind G5 >RUN:G5;\n"), "{sent}");
        assert!(!sent.contains("lights"), "{sent}");
        // Hand-counted notes: one per half; the control character also orphans the bind.
        for (half, notes_expected) in [
            ("# run G5 foo\n", 1),
            ("bind G5 >RUN:G5;\n", 1),
            ("# run G5\n", 1),
            ("# run G5 a\u{7}b\nbind G5 >RUN:G5;\n", 2),
            // A mode with a release half needs the two-group bind; the mode word alone
            // names no program.
            ("# run G7 hold arecord\nbind G7 >RUN:G7;\n", 1),
            ("# run G7 hold\n", 1),
            ("# run G7 both arecord\nbind G7 >RUN:G7; RELEASE:G7;\n", 0),
            ("# run G7 arecord\nbind G7 >RUN:G7; RELEASE:G7;\n", 0),
        ] {
            assert_eq!(Profile::parse(half).1.len(), notes_expected, "{half:?}");
        }
        // A held program round-trips with its mode word and its two-group bind.
        let text = "# run G7 hold arecord -f cd ~/clip.wav\nbind G7 >RUN:G7; RELEASE:G7;\n";
        let (p, notes) = Profile::parse(text);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(p.runs["G7"].when, crate::run::When::Hold);
        let again = p.to_text();
        assert!(again.contains(text), "{again}");
        assert_eq!(Profile::parse(&again).0, p);
    }
    #[test]
    fn round_trip_and_stickmode_drop() {
        let (p, notes) = Profile::parse(
            "# c\n# lcd my pic\nrgb 31 0 127\nbind G1 KEY_ENTER\nbind G2 KEY_LEFTCTRL+KEY_X\n\
             bind STICK_UP KEY_V\nstickmode KEYS\nmod 5\nfont 5x8\nbind G3 KEY_A KEY_B\n",
        );
        assert_eq!(notes.len(), 1);
        assert_eq!(p.rgb, Some([31, 0, 127]));
        assert_eq!(p.leds, Some(5));
        assert_eq!(p.lcd.as_deref(), Some("my pic"));
        assert_eq!(p.binds["G3"], "KEY_A KEY_B");
        assert_eq!(p.extra, vec!["font 5x8"]);
        let (again, _) = Profile::parse(&p.to_text());
        assert_eq!(again, p);
    }
    /// Before 0.2.3x the meter was a picture name (`# lcd health cs2`). Such a file reads
    /// as the health display with no picture of its own; a live display chosen beside it
    /// wins, and the stale name is dropped either way.
    #[test]
    fn old_meter_picture_names_migrate_to_the_health_line() {
        use crate::meter::Reader;
        for (text, display) in [
            ("# lcd health log\n", Display::Health(Reader::Log)),
            ("# lcd health cs2\n", Display::Health(Reader::Cs2)),
            ("# lcd health\n", Display::Health(Reader::Feed)),
            ("# lcd health log\n# pressure on", Display::Pressure),
            ("# lcd health log\n# mpv on", Display::Mpv),
            (
                "# health feed\n# lcd health cs2",
                Display::Health(Reader::Feed),
            ),
        ] {
            let (p, _) = Profile::parse(text);
            assert_eq!(p.display, display, "{text:?}");
            assert_eq!(p.lcd, None, "{text:?}");
            let written = p.to_text();
            assert!(!written.contains("# lcd "), "{written}");
            assert_eq!(Profile::parse(&written).0, p);
        }
        // A real picture beside the health line is kept.
        let (p, notes) = Profile::parse("# lcd my pic\n# health cs2\n");
        assert!(notes.is_empty());
        assert_eq!(p.display, Display::Health(Reader::Cs2));
        assert_eq!(p.lcd.as_deref(), Some("my pic"));
        assert!(p.to_text().contains("# health cs2\n"));
    }
    #[test]
    fn stick_preferences_are_metadata_and_legacy_profiles_preserve_the_mode() {
        for (text, expected) in [
            ("", None),
            ("# stick analog", Some(StickMode::Analog)),
            ("# stick keys", Some(StickMode::Keys)),
            ("# stick current", None),
        ] {
            let (p, notes) = Profile::parse(text);
            assert!(notes.is_empty());
            assert_eq!(p.stick, expected);
            assert_eq!(Profile::parse(&p.to_text()).0, p);
            assert!(!p.commands(None, true).contains("stick"));
        }
        let (p, notes) = Profile::parse("# stick keys\n# stick nonsense\nstickmode ABSOLUTE");
        assert_eq!(p.stick, Some(StickMode::Keys));
        assert_eq!(notes.len(), 2);
        assert_eq!(p.commands(None, false), "bind TOP KEY_RESERVED\n");
    }
    #[test]
    fn commands_unbind_against_base_and_skip_top_when_analog() {
        let (base, _) = Profile::parse("bind G1 KEY_A\nbind G2 KEY_B\nbind TOP KEY_C\n");
        let (p, _) = Profile::parse("bind G1 KEY_Q\nbind TOP KEY_D\n");
        let c = p.commands(Some(&base), true);
        assert_eq!(c, "bind G1 KEY_Q\nbind G2 KEY_RESERVED\n");
        assert!(p.commands(Some(&base), false).contains("bind TOP KEY_D\n"));
        assert_eq!(p.commands(None, false), "bind G1 KEY_Q\nbind TOP KEY_D\n");
    }
    #[test]
    fn chords() {
        assert_eq!(
            chord("KEY_LEFTCTRL+KEY_X"),
            Some((vec!["LEFTCTRL".into()], Some("X".into())))
        );
        assert_eq!(
            chord("KEY_LEFTSHIFT"),
            Some((vec!["LEFTSHIFT".into()], None))
        );
        assert_eq!(chord("KEY_A KEY_B"), None);
        assert_eq!(chord(">hello"), None);
        assert_eq!(chord("KEY_A+KEY_B"), None);
        assert_eq!(chord("-KEY_A"), None);
        assert_eq!(chord("MLEFT"), None);
        assert_eq!(chord("KEY_LEFTCTRL+MLEFT"), None);
        assert_eq!(chord("KEY_RESERVED"), None);
        assert_eq!(chord("KEY_BRIGHTNESS_MIN"), None); // code 592, above the uinput range
        assert_eq!(
            chord_action(&["LEFTALT".into(), "LEFTCTRL".into()], Some("F4")),
            "KEY_LEFTCTRL+KEY_LEFTALT+KEY_F4"
        );
    }
}
