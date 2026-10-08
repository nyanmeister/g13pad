// SPDX-License-Identifier: GPL-3.0-or-later
//! Short-lived panel status output; no GUI, service calls, or device writes.

const MARK: &str = include_str!("../panel/g13.svg");

fn fields(name: &str, up: bool) -> (String, String) {
    let text = match (name, up) {
        ("default", true) => "G13".to_string(),
        (n, true) => format!("G13·{n}"),
        ("default", false) => "G13 ✗".to_string(),
        (n, false) => format!("G13·{n} ✗"),
    };
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
    let (text, tooltip) = fields(&crate::active_name(), up);
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

fn xfce_output(name: &str, up: bool, executable: &str) -> Result<String, String> {
    // Genmon extracts its action tags literally rather than using an XML parser.
    // Quote the executable for GLib's command-line parser, but don't entity-encode it.
    if executable.contains(['<', '>', '\n', '\r']) {
        return Err("XFCE panel action cannot represent this executable path".into());
    }
    let action = format!("'{}' edit", executable.replace('\'', "'\\''"));
    let (text, tooltip) = fields(name, up);
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
    xfce_output(&crate::active_name(), crate::daemon::up(), executable)
}

/// Plain status for command panels such as i3blocks, Polybar and tint2.
pub fn text() -> String {
    let name = crate::active_name();
    let state = if crate::daemon::up() {
        "connected"
    } else {
        "disconnected"
    };
    format!("G13 · {name} · {state}")
}

/// Waybar's native JSON custom-module contract. Serde escapes arbitrary profile names.
pub fn waybar() -> String {
    let name = crate::active_name();
    let up = crate::daemon::up();
    let (text, tooltip) = fields(&name, up);
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
    fn xfce_names_cannot_inject_markup_or_actions() {
        let output = xfce_output(
            "<txtclick>bad & 'quoted'</txtclick>",
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
        let connected = xfce_output("default", true, path).unwrap();
        assert!(connected.contains("<icon>g13pad</icon>"));
        assert!(connected.contains("<txt>G13</txt>"));
        assert!(connected.contains("'/opt/G13 Pad & user'\\''s/bin/g13map' edit"));
        let disconnected = xfce_output("ゲーム", false, path).unwrap();
        assert!(disconnected.contains("<icon>g13pad-disconnected</icon>"));
        assert!(disconnected.contains("13·ゲーム ✗"));
        assert!(disconnected.contains("Daemon pipe missing"));
        assert!(xfce_output("default", true, "/opt/<bad>/g13map").is_err());
    }
}
