// SPDX-License-Identifier: GPL-3.0-or-later
//! The graphical session from a process that may have started before it: a user unit at
//! login runs with no DISPLAY, XAUTHORITY or I3SOCK, and systemd starts it before the
//! desktop has exported them (LXQt and i3 never reach graphical-session.target). The user
//! manager's environment gets them later, so look there when the process's own is bare.
//! No username, display number or authority file is assumed.
use std::process::Command;

/// A session variable: this process's environment, else the user manager's.
pub fn var(name: &str) -> Option<String> {
    if let Some(v) = std::env::var_os(name) {
        return v.into_string().ok();
    }
    let out = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_environment(&String::from_utf8_lossy(&out.stdout), name)
}

/// `NAME=value` lines as `systemctl --user show-environment` prints them (a value with
/// special characters comes shell-quoted; the plain forms are what a display or a path uses).
fn parse_environment(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        if k != name {
            return None;
        }
        let v = v
            .strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .or_else(|| v.strip_prefix("$'").and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// What `detach_pointer` did.
#[derive(Debug, PartialEq)]
pub enum Detach {
    /// No display in this environment or the user manager's yet: nothing could be inspected.
    NoDisplay,
    /// The display has no `pointer:G13` (a fresh libinput already suppresses it), or the
    /// isolated test session asked for no unit work.
    Absent,
    Detached(u32),
}

impl std::fmt::Display for Detach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Detach::NoDisplay => write!(
                f,
                "no DISPLAY here or in the user manager's environment; the G13 source pointer \
                 was not detached (g13map watch retries once the session has one)"
            ),
            Detach::Absent => Ok(()),
            Detach::Detached(id) => {
                write!(
                    f,
                    "detached G13 source pointer {id}; keyboard remains attached"
                )
            }
        }
    }
}

/// Floats the G13's source pointer (the stick as X11 sees it) so it cannot move the desktop
/// pointer; the adapter reads the event device directly. Only `pointer:G13`, only when it is
/// one device; the keyboard half stays attached.
pub fn detach_pointer() -> Result<Detach, String> {
    if std::env::var("G13MAP_UNIT").is_ok_and(|v| v == "0") {
        return Ok(Detach::Absent);
    }
    let Some(display) = var("DISPLAY") else {
        return Ok(Detach::NoDisplay);
    };
    let xinput = |args: &[&str]| {
        let mut c = Command::new("/usr/bin/timeout");
        c.arg("3").arg("xinput").args(args).env("DISPLAY", &display);
        if let Some(auth) = var("XAUTHORITY") {
            c.env("XAUTHORITY", auth);
        }
        c.output()
    };
    let output = xinput(&["list", "--id-only", "pointer:G13"])
        .map_err(|e| format!("cannot inspect the G13 source pointer: {e}"))?;
    if !output.status.success() {
        if output.status.code() == Some(124) {
            return Err("timed out inspecting the G13 source pointer".into());
        }
        return Ok(Detach::Absent);
    }
    let id = pointer_id(&String::from_utf8_lossy(&output.stdout))?;
    let floated = xinput(&["float", &id.to_string()])
        .map_err(|e| format!("cannot detach the G13 source pointer: {e}"))?;
    if !floated.status.success() {
        return Err(format!(
            "cannot detach G13 source pointer {id}: {}",
            floated.status
        ));
    }
    Ok(Detach::Detached(id))
}

fn pointer_id(text: &str) -> Result<u32, String> {
    let mut fields = text.split_whitespace();
    let id = fields
        .next()
        .and_then(|id| id.parse::<u32>().ok())
        .filter(|id| *id > 0);
    if fields.next().is_some() || id.is_none() {
        return Err("G13 pointer identity is ambiguous; refusing to detach".into());
    }
    Ok(id.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_ambiguous_or_non_numeric_pointer_targets() {
        assert_eq!(pointer_id("24\n").unwrap(), 24);
        for text in ["", "0", "24\n25\n", "pointer:G13", "24;other"] {
            assert!(pointer_id(text).is_err());
        }
    }

    #[test]
    fn manager_environment_lines_parse_plain_and_quoted_values() {
        let text = "DISPLAY=:0\nI3SOCK=/run/user/1000/i3/ipc-socket.1964\nEMPTY=\n\
                    XAUTHORITY='/home/me/My Auth'\nODD=$'a\\tb'\nNODISPLAY=:1\n";
        assert_eq!(parse_environment(text, "DISPLAY").as_deref(), Some(":0"));
        assert_eq!(
            parse_environment(text, "I3SOCK").as_deref(),
            Some("/run/user/1000/i3/ipc-socket.1964")
        );
        assert_eq!(
            parse_environment(text, "XAUTHORITY").as_deref(),
            Some("/home/me/My Auth")
        );
        assert_eq!(parse_environment(text, "ODD").as_deref(), Some("a\\tb"));
        assert_eq!(parse_environment(text, "EMPTY"), None);
        assert_eq!(parse_environment(text, "MISSING"), None);
        assert_eq!(parse_environment("", "DISPLAY"), None);
    }

    #[test]
    fn detach_reports_a_bare_environment_instead_of_guessing() {
        // The test binary runs under cargo: DISPLAY may exist. Only the unit-off path is
        // environment-independent here; the NoDisplay path is exercised by the watcher test.
        std::env::set_var("G13MAP_UNIT", "0");
        assert_eq!(detach_pointer().unwrap(), Detach::Absent);
        std::env::remove_var("G13MAP_UNIT");
        assert_eq!(Detach::Absent.to_string(), "");
        assert!(Detach::NoDisplay.to_string().contains("not detached"));
        assert_eq!(
            Detach::Detached(7).to_string(),
            "detached G13 source pointer 7; keyboard remains attached"
        );
    }
}
