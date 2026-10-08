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
    /// None preserves the current mode. A comment so the service remains its only owner.
    pub stick: Option<StickMode>,
    pub gamepad: Option<crate::gamepad::Mapping>,
    /// Other command lines, kept verbatim (stickzone bounds, font, ...).
    pub extra: Vec<String>,
}

impl Profile {
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
        if let Some(mode) = self.stick {
            out.push_str(&format!("# stick {}\n", mode.label()));
        }
        if let Some(mapping) = self.gamepad {
            out.push_str(&format!("# gamepad {}\n", mapping.compact()));
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
