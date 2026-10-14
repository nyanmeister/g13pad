// SPDX-License-Identifier: GPL-3.0-or-later
//! `g13map doctor`: the pad's services and files, one line each, with the fix when one
//! is wrong. Written 2026-10-14 after the adapter spun for a night on a vanished node:
//! nothing without root said so, and this would have. Reads only; never restarts.
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

pub const SOURCE: &str = "/dev/input/g13-analog-source";
pub const SOURCE_UNIT: &str = "dev-input-g13\\x2danalog\\x2dsource.device";

/// One check: what it looked at, and the fix if it is wrong.
struct Line {
    what: String,
    fix: Option<String>,
}

fn ok(what: impl Into<String>) -> Line {
    Line {
        what: what.into(),
        fix: None,
    }
}

fn fix(what: impl Into<String>, how: impl Into<String>) -> Line {
    Line {
        what: what.into(),
        fix: Some(how.into()),
    }
}

/// `systemctl show` properties, in the order asked for (systemctl prints them in its
/// own, so `--value` alone would not do); `None` when systemctl fails or one is missing.
fn show(user: bool, unit: &str, props: &[&str]) -> Option<Vec<String>> {
    let mut c = Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    let o = c
        .args(["show", "--timestamp=unix", "-p"])
        .arg(props.join(","))
        .arg(unit)
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&o.stdout);
    pick(&text, props)
}

/// The values of `props` out of `Key=value` lines, in the order of `props`.
fn pick(text: &str, props: &[&str]) -> Option<Vec<String>> {
    props
        .iter()
        .map(|p| {
            text.lines()
                .find_map(|l| l.strip_prefix(p).and_then(|r| r.strip_prefix('=')))
                .map(str::to_string)
        })
        .collect()
}

/// A `--timestamp=unix` value (`@1791976734`); empty when the unit never ran.
pub fn unix_stamp(s: &str) -> Option<i64> {
    s.trim().strip_prefix('@')?.parse().ok()
}

/// The adapter reads the node the symlink pointed at when it started. If the symlink was
/// made after that (the pad reconnected and the driver rebuilt its device), the adapter
/// is on a vanished node: no stick, and xboxdrv spins. Hand-derived: started at 100,
/// symlink at 100 is fine (same second), at 101 is stale.
pub fn stale(adapter_started: Option<i64>, source_made: Option<i64>) -> bool {
    matches!((adapter_started, source_made), (Some(a), Some(s)) if s > a)
}

fn checks() -> Vec<Line> {
    let mut out = Vec::new();
    // The driver and its pipe.
    match show(false, "g13.service", &["ActiveState", "MainPID"]) {
        Some(v) if v[0] == "active" => {
            out.push(ok(format!("g13.service active, g13d pid {}", v[1])))
        }
        Some(v) => out.push(fix(
            format!("g13.service is {}", v[0]),
            "sudo systemctl start g13.service; journalctl -u g13.service for why it stopped",
        )),
        None => out.push(fix(
            "g13.service unknown to systemd",
            "is the package installed?",
        )),
    }
    let pipe = crate::daemon::pipe_path();
    if crate::daemon::up() {
        out.push(ok(format!("driver pipe {} is being read", pipe.display())));
    } else if pipe.exists() {
        out.push(fix(
            format!("driver pipe {} exists but nothing reads it", pipe.display()),
            "g13d is not running or is stuck; sudo systemctl restart g13.service",
        ));
    } else {
        out.push(fix(
            format!("driver pipe {} is missing", pipe.display()),
            "g13d has not started, or its RuntimeDirectory is gone",
        ));
    }
    // The stick's source node, its device unit and the adapter on it.
    let source = Path::new(SOURCE);
    let source_made = std::fs::symlink_metadata(source).ok().map(|m| m.ctime());
    match (source_made, std::fs::metadata(source).is_ok()) {
        (Some(_), true) => out.push(ok(format!(
            "{SOURCE} -> {}",
            std::fs::read_link(source)
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ))),
        (Some(_), false) => out.push(fix(
            format!("{SOURCE} points at a node that is gone"),
            "udev will replace it on the next reconnect; replug the pad",
        )),
        (None, _) => out.push(fix(
            format!("{SOURCE} is missing"),
            "the pad is not connected, or the driver has not opened it (journalctl -u g13.service)",
        )),
    }
    if source_made.is_some() {
        match show(false, SOURCE_UNIT, &["ActiveState"]) {
            Some(v) if v[0] == "active" => out.push(ok(format!("{SOURCE_UNIT} plugged"))),
            _ => out.push(fix(
                format!("{SOURCE_UNIT} is not plugged, so g13-analog.service cannot bind to it"),
                "the udev rule did not tag the node (an older rule file?): sudo udevadm control --reload; \
                 sudo udevadm trigger --action=add --subsystem-match=input --attr-match=name=G13, or replug the pad",
            )),
        }
    }
    let adapter = show(
        false,
        "g13-analog.service",
        &["ActiveState", "ExecMainStartTimestamp", "MainPID"],
    );
    match &adapter {
        Some(v) if v[0] == "active" => {
            let started = unix_stamp(&v[1]);
            if stale(started, source_made) {
                out.push(fix(
                    format!(
                        "g13-analog.service (xboxdrv pid {}) started before the current {SOURCE}: it is on a vanished node",
                        v[2]
                    ),
                    "sudo systemctl restart g13-analog.service (the stick is dead and xboxdrv is spinning until then)",
                ));
            } else {
                out.push(ok(format!(
                    "g13-analog.service active, xboxdrv pid {}, on the current source",
                    v[2]
                )));
            }
        }
        Some(v) => out.push(ok(format!(
            "g13-analog.service {} (the profile decides)",
            v[0]
        ))),
        None => out.push(fix(
            "g13-analog.service unknown to systemd",
            "is the package installed?",
        )),
    }
    // The profile, the stick it wants, and the watcher it needs.
    let name = crate::active_name();
    match crate::load(&name) {
        Ok((p, notes)) => {
            out.push(ok(format!("profile '{name}' loads")));
            for n in notes {
                out.push(fix(
                    format!("profile '{name}': {n}"),
                    "repair the line in the editor or g13map profile",
                ));
            }
            let analog = crate::daemon::analog_active();
            match p.stick {
                Some(crate::profile::StickMode::Analog) if !analog => out.push(fix(
                    format!("profile '{name}' wants the stick analog but g13-analog.service is not active"),
                    "g13map apply (the watcher starts it through sudo); check the sudoers rule if that fails",
                )),
                Some(crate::profile::StickMode::Keys) if analog => out.push(fix(
                    format!("profile '{name}' wants the stick as keys but g13-analog.service is active"),
                    "g13map apply",
                )),
                _ => out.push(ok(format!(
                    "stick: profile says {}, adapter is {}",
                    p.stick.map(|s| s.label()).unwrap_or("current"),
                    if analog { "active" } else { "inactive" }
                ))),
            }
            let watcher =
                show(true, "g13map-watch.service", &["ActiveState"]).map(|v| v[0].clone());
            match watcher.as_deref() {
                Some("active") => out.push(ok("g13map-watch.service (user) active")),
                Some(state) if p.needs_watcher() => out.push(fix(
                    format!("g13map-watch.service (user) is {state} and profile '{name}' needs it"),
                    "systemctl --user enable --now g13map-watch.service",
                )),
                Some(state) => out.push(ok(format!(
                    "g13map-watch.service (user) {state}; profile '{name}' does not need it"
                ))),
                None => out.push(fix(
                    "no user manager answers for g13map-watch.service",
                    "run this from the login session",
                )),
            }
        }
        Err(e) => out.push(fix(
            format!("profile '{name}': {e}"),
            "g13map profiles; g13map use NAME",
        )),
    }
    if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        out.push(ok(
            "a display is in the environment (window rules can see focus)",
        ));
    } else {
        out.push(ok(
            "no DISPLAY or WAYLAND_DISPLAY here (window rules need the session's)",
        ));
    }
    out
}

/// Prints the report; `Err` (and so exit status 1) names how many lines need fixing.
pub fn run() -> Result<String, String> {
    let lines = checks();
    let mut problems = 0;
    for l in &lines {
        match &l.fix {
            None => println!("ok   {}", l.what),
            Some(how) => {
                problems += 1;
                println!("FIX  {}\n     {how}", l.what);
            }
        }
    }
    if problems == 0 {
        Ok("everything the pad needs is in place".into())
    } else {
        Err(format!("{problems} to fix"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_made_after_the_adapter_started_is_stale() {
        assert_eq!(unix_stamp("@1791976734\n"), Some(1791976734));
        assert_eq!(unix_stamp(""), None, "a unit that never ran");
        assert_eq!(unix_stamp("Wed 2026-10-14 04:18:54 PDT"), None);
        assert!(
            !stale(Some(100), Some(100)),
            "the same second is the same reconnect"
        );
        assert!(!stale(Some(100), Some(99)));
        assert!(stale(Some(100), Some(101)));
        assert!(!stale(None, Some(101)), "no adapter, nothing to be stale");
        assert!(!stale(Some(100), None));
        // The night of 2026-10-14: the adapter from 03:34 the day before, the node from
        // 04:10:54.
        assert!(stale(Some(1791887685), Some(1791976254)));
    }

    /// systemctl prints properties in its own order; the picker returns ours.
    #[test]
    fn properties_come_back_in_the_order_asked_for() {
        let text = "MainPID=232795\nExecMainStartTimestamp=@1791976734\nActiveState=active\n";
        assert_eq!(
            pick(text, &["ActiveState", "ExecMainStartTimestamp", "MainPID"]),
            Some(vec!["active".into(), "@1791976734".into(), "232795".into()])
        );
        assert_eq!(pick(text, &["ActiveState", "Nope"]), None);
        assert_eq!(
            pick("ActiveStateX=1\nActiveState=inactive\n", &["ActiveState"]),
            Some(vec!["inactive".into()]),
            "a prefix is not a match"
        );
    }
}
