// SPDX-License-Identifier: GPL-3.0-or-later
//! Short-lived panel status output; no GUI, service calls, or device writes.
//!
//! Beside the device mark (LXQt, XFCE) the text is the active profile's name alone: the
//! mark says "G13" (asked 2026-10-27; it used to read `G13·name`). Formats without the
//! mark (Waybar's JSON, plain text) keep the `G13` word. `~/.config/g13map/panel` can turn
//! the name off, leaving the mark by itself.
use std::{fs, path::PathBuf};

const MARK: &str = include_str!("../panel/g13.svg");

/// The applet's own settings: whether the active profile's name is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub show_name: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { show_name: true }
    }
}

pub fn path() -> PathBuf {
    crate::config_dir().join("panel")
}

impl Settings {
    pub fn load() -> Settings {
        fs::read_to_string(path())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }
    /// `name on|off` lines; comments, blanks and anything else are ignored.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            match line.split_whitespace().collect::<Vec<_>>().as_slice() {
                ["name", "on"] => s.show_name = true,
                ["name", "off"] => s.show_name = false,
                _ => {}
            }
        }
        s
    }
    pub fn to_text(self) -> String {
        format!(
            "# g13map: the panel applet.\n\
             # `name on|off`: show the active profile's name beside the device mark.\n\
             name {}\n",
            if self.show_name { "on" } else { "off" }
        )
    }
    pub fn save(&self) -> Result<(), String> {
        let p = path();
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&p, self.to_text()).map_err(|e| format!("{}: {e}", p.display()))
    }
}

/// `g13map panel name [on|off]`.
pub fn name_command(args: &[&str]) -> Result<String, String> {
    let s = Settings::load();
    match args {
        [] => Ok(if s.show_name { "on" } else { "off" }.into()),
        ["on"] | ["off"] => {
            let on = args[0] == "on";
            Settings { show_name: on }.save()?;
            Ok(if on {
                "the panel applet shows the profile name".into()
            } else {
                "the panel applet shows the mark alone".into()
            })
        }
        _ => Err("usage: g13map panel name [on|off]".into()),
    }
}

/// The label and tooltip. `marked`: the format draws the device mark beside the text, so
/// the text is the name alone (or nothing); otherwise `G13` leads. `show`: the name at all.
fn fields(name: &str, up: bool, show: bool, marked: bool) -> (String, String) {
    let named = show && name != "default";
    let mut text = match (marked, named) {
        (true, true) => name.to_string(),
        (true, false) => String::new(),
        (false, true) => format!("G13·{name}"),
        (false, false) => "G13".to_string(),
    };
    if !up {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push('✗');
    }
    let state = if up {
        ""
    } else {
        " Daemon pipe missing: is g13.service running?"
    };
    let tooltip = format!("G13 key bindings — profile “{name}”.{state} Click for the editor.");
    (text, tooltip)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Existing LXQt custom-command format (outputFormat=2).
pub fn lxqt() -> String {
    let up = crate::daemon::up();
    let (text, tooltip) = fields(&crate::active_name(), up, Settings::load().show_name, true);
    let colour = if up { "#00B8FC" } else { "#8A8A8A" };
    format!(
        "text:{} icon:{} tooltip:{}",
        base64(text.as_bytes()),
        base64(MARK.replace("COLOUR", colour).as_bytes()),
        base64(tooltip.as_bytes())
    )
}

fn markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xfce_output(name: &str, up: bool, show: bool, executable: &str) -> Result<String, String> {
    // Genmon extracts its action tags literally rather than using an XML parser.
    // Quote the executable for GLib's command-line parser, but don't entity-encode it.
    if executable.contains(['<', '>', '\n', '\r']) {
        return Err("XFCE panel action cannot represent this executable path".into());
    }
    let action = format!("'{}' edit", executable.replace('\'', "'\\''"));
    let (text, tooltip) = fields(name, up, show, true);
    let icon = if up { "g13pad" } else { "g13pad-disconnected" };
    Ok(format!(
        "<icon>{icon}</icon><iconclick>{action}</iconclick>\
         <txt>{}</txt><txtclick>{action}</txtclick><tool>{}</tool>",
        markup(&text),
        markup(&tooltip)
    ))
}

/// XFCE Generic Monitor output; icon and text both open this executable's editor.
pub fn xfce() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = executable
        .to_str()
        .ok_or("XFCE panel requires a UTF-8 executable path")?;
    xfce_output(
        &crate::active_name(),
        crate::daemon::up(),
        Settings::load().show_name,
        executable,
    )
}

/// Plain status for command panels such as i3blocks, Polybar and tint2.
pub fn text() -> String {
    let name = crate::active_name();
    let state = if crate::daemon::up() {
        "connected"
    } else {
        "disconnected"
    };
    if Settings::load().show_name {
        format!("G13 · {name} · {state}")
    } else {
        format!("G13 · {state}")
    }
}

/// Waybar's native JSON custom-module contract. Serde escapes arbitrary profile names.
pub fn waybar() -> String {
    let name = crate::active_name();
    let up = crate::daemon::up();
    let (text, tooltip) = fields(&name, up, Settings::load().show_name, false);
    serde_json::json!({ "text": text, "tooltip": tooltip,
        "class": if up { "connected" } else { "disconnected" }, "alt": "g13pad" })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("13·drg ✗".as_bytes()), "MTPCt2RyZyDinJc=");
    }

    #[test]
    fn the_text_is_the_name_beside_the_mark_and_g13_without_it() {
        // Hand-derived table: (name, up, show, marked) -> text.
        for (name, up, show, marked, want) in [
            ("drg", true, true, true, "drg"),
            ("drg", false, true, true, "drg ✗"),
            ("default", true, true, true, ""),
            ("default", false, true, true, "✗"),
            ("drg", true, false, true, ""),
            ("drg", false, false, true, "✗"),
            ("drg", true, true, false, "G13·drg"),
            ("drg", false, true, false, "G13·drg ✗"),
            ("default", true, true, false, "G13"),
            ("drg", false, false, false, "G13 ✗"),
        ] {
            let (text, tooltip) = fields(name, up, show, marked);
            assert_eq!(text, want, "{name} up={up} show={show} marked={marked}");
            assert!(
                tooltip.contains(&format!("“{name}”")),
                "the tooltip always names it"
            );
            assert_eq!(tooltip.contains("Daemon pipe missing"), !up);
        }
    }

    #[test]
    fn settings_parse_and_round_trip() {
        assert!(Settings::parse("").show_name);
        assert!(!Settings::parse("# c\nname off\n").show_name);
        assert!(Settings::parse("name off\nname on\n").show_name);
        assert!(Settings::parse("name maybe\nnom off\n").show_name);
        let off = Settings { show_name: false };
        assert_eq!(Settings::parse(&off.to_text()), off);
        assert_eq!(
            Settings::parse(&Settings::default().to_text()),
            Settings::default()
        );
    }

    #[test]
    fn xfce_names_cannot_inject_markup_or_actions() {
        let output = xfce_output(
            "<txtclick>bad & 'quoted'</txtclick>",
            true,
            true,
            "/usr/bin/g13map",
        )
        .unwrap();
        assert_eq!(output.matches("<txtclick>").count(), 1);
        assert_eq!(output.matches("<iconclick>").count(), 1);
        assert!(output.contains("&lt;txtclick&gt;bad &amp; &apos;quoted&apos;&lt;/txtclick&gt;"));
        assert!(output.contains("<txtclick>'/usr/bin/g13map' edit</txtclick>"));
    }

    #[test]
    fn xfce_state_and_action_follow_the_actual_executable() {
        let path = "/opt/G13 Pad & user's/bin/g13map";
        let connected = xfce_output("default", true, true, path).unwrap();
        assert!(connected.contains("<icon>g13pad</icon>"));
        assert!(
            connected.contains("<txt></txt>"),
            "the mark alone for the default profile"
        );
        assert!(connected.contains("'/opt/G13 Pad & user'\\''s/bin/g13map' edit"));
        let disconnected = xfce_output("ゲーム", false, true, path).unwrap();
        assert!(disconnected.contains("<icon>g13pad-disconnected</icon>"));
        assert!(disconnected.contains("<txt>ゲーム ✗</txt>"));
        assert!(disconnected.contains("Daemon pipe missing"));
        let unnamed = xfce_output("ゲーム", true, false, path).unwrap();
        assert!(unnamed.contains("<txt></txt>") && unnamed.contains("“ゲーム”"));
        assert!(xfce_output("default", true, true, "/opt/<bad>/g13map").is_err());
    }
}
