// SPDX-License-Identifier: GPL-3.0-or-later
//! g13map: a key-binding editor and panel applet for the Logitech G13 under g13d.
//!
//!   g13map           panel output: device mark, G13 plus profile, live tooltip
//!   g13map edit      the editor window
//!   g13map apply     send the active profile to the running daemon (login service)
//!   g13map watch     switch profiles: M-keys (daemon output pipe), i3 focus (user service)
//!   g13map import [FILE] [NAME]   copy a bind file into the profiles (default: the daemon's)
#[cfg(feature = "editor")]
mod adjust;
mod application;
#[cfg(feature = "art")]
pub mod art;
#[cfg(feature = "editor")]
mod board;
mod cli;
mod daemon;
mod focus;
#[cfg(feature = "editor")]
mod fonts;
#[cfg(all(test, feature = "editor"))]
mod fuzz_tests;
#[cfg(feature = "editor")]
mod gui;
pub mod keys;
pub mod lcd;
#[cfg(feature = "editor")]
mod marquee;
mod modes;
pub mod overlay;
mod panel;
pub mod profile;
mod session;
#[cfg(test)]
mod test_support;
#[cfg(feature = "editor")]
pub mod text_options;

use g13pad_core::gamepad;
use profile::Profile;
use std::{env, fs, path::PathBuf};

/// Names become individual files and single-line metadata, never paths or commands.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || name == "."
        || name == ".."
        || name.len() > 200
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err("name must be 1–200 bytes, with no path separators, control characters or surrounding whitespace".into());
    }
    Ok(())
}

pub const DAEMON_CONFIG: &str = "/etc/g13/default.bind";

/// The daemon's startup config, the baseline that profile transitions diff against.
/// `G13MAP_DAEMON_CONFIG` overrides it; the test sandbox points it at the packaged file so
/// the suite does not depend on an installed driver (found by the first CI run).
pub fn daemon_config() -> PathBuf {
    env::var_os("G13MAP_DAEMON_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DAEMON_CONFIG))
}

pub fn config_dir() -> PathBuf {
    env::var_os("G13MAP_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("g13map")))
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config/g13map")
        })
}

pub fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

pub fn profile_path(name: &str) -> PathBuf {
    profiles_dir().join(format!("{name}.bind"))
}

pub fn profile_names() -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(profiles_dir())
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| {
                    e.path()
                        .file_stem()?
                        .to_str()
                        .map(String::from)
                        .filter(|_| e.path().extension().is_some_and(|x| x == "bind"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn active_name() -> String {
    fs::read_to_string(config_dir().join("active"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".into())
}

pub fn set_active(name: &str) -> Result<(), String> {
    validate_name(name)?;
    fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    fs::write(config_dir().join("active"), format!("{name}\n")).map_err(|e| e.to_string())
}

pub fn load(name: &str) -> Result<(Profile, Vec<String>), String> {
    validate_name(name)?;
    let path = profile_path(name);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Profile::parse(&text))
}

/// Atomic save: write beside, then rename.
pub fn save(name: &str, p: &Profile) -> Result<(), String> {
    validate_name(name)?;
    fs::create_dir_all(profiles_dir()).map_err(|e| e.to_string())?;
    let path = profile_path(name);
    let tmp = path.with_extension("bind.new");
    fs::write(&tmp, p.to_text())
        .and_then(|_| fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The daemon's startup config, the baseline `apply` diffs against.
pub fn daemon_base() -> Option<Profile> {
    fs::read_to_string(daemon_config())
        .ok()
        .map(|t| Profile::parse(&t).0)
}

fn import(file: Option<&str>, name: Option<&str>) -> Result<String, String> {
    let default = daemon_config();
    let file = file.unwrap_or_else(|| default.to_str().unwrap_or(DAEMON_CONFIG));
    let name = name.unwrap_or("default");
    validate_name(name)?;
    if profile_path(name).exists() {
        return Err(format!(
            "profile '{name}' already exists ({})",
            profile_path(name).display()
        ));
    }
    let text = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let (p, notes) = Profile::parse(&text);
    save(name, &p)?;
    let mut msg = format!(
        "imported {file} as profile '{name}' ({} bindings)",
        p.binds.len()
    );
    for n in notes {
        msg.push_str(&format!("\n  {n}"));
    }
    Ok(msg)
}

/// Makes sure a profile exists before the editor or apply runs; first run imports the daemon's.
/// An `active` file naming a profile that is gone falls back to the first one there is,
/// rather than leaving the applet's click with nothing to open.
fn ensure_profile() -> Result<String, String> {
    let name = active_name();
    validate_name(&name)?;
    if profile_path(&name).exists() {
        return Ok(name);
    }
    match profile_names().first() {
        None => {
            import(None, Some(&name))?;
            Ok(name)
        }
        Some(first) => {
            eprintln!("g13map: active profile '{name}' is missing; using '{first}'");
            set_active(first)?;
            Ok(first.clone())
        }
    }
}

fn apply() -> Result<String, String> {
    let name = ensure_profile()?;
    let (p, _) = load(&name)?;
    let baseline = daemon_base();
    let plan = application::Plan {
        target: &p,
        previous: None,
        baseline: baseline.as_ref(),
        routing: if modes::Modes::load().on {
            application::Routing::Routes
        } else {
            application::Routing::Profile
        },
    };
    let analog = plan.execute_complete()?;
    let cmds = plan.complete_commands(analog);
    let mut msg = format!("applied profile '{name}': {} lines", cmds.lines().count());
    if let Some(image) = &p.lcd {
        // The daemon starts with its logo; a profile without an image leaves that alone.
        // An animation gets its first frame here; `g13map watch` plays the rest.
        drop(lcd::show(Some(image), false)?);
        let frames = lcd::frame_count(image);
        if frames > 1 {
            msg.push_str(&format!(
                ", LCD animation '{image}' ({frames} frames; g13map watch plays it)"
            ));
        } else {
            msg.push_str(&format!(", LCD image '{image}'"));
        }
    }
    Ok(msg)
}

pub fn run() {
    finish(cli::dispatch(&env::args().skip(1).collect::<Vec<_>>()));
}

#[cfg(feature = "editor")]
pub fn run_editor() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => ensure_profile().and_then(gui::run),
        [arg] if arg == "--version" => Ok(format!(
            "g13pad {} (g13map-editor)",
            env!("CARGO_PKG_VERSION")
        )),
        [arg] if arg == "--help" || arg == "-h" => {
            Ok("usage: g13map-editor [--version|--help]\nNormally launch with: g13map edit".into())
        }
        [arg, text] if arg == "--error-frame" => {
            use std::io::Write;
            let result = overlay::error_frame(text).and_then(|bytes| {
                std::io::stdout()
                    .write_all(&bytes)
                    .map_err(|e| e.to_string())
            });
            finish(result.map(|_| String::new()));
            return;
        }
        [arg, text, rest @ ..] if arg == "--marquee" && rest.len() <= 1 => {
            if let Some(name) = rest.first() {
                finish(validate_name(name).map(|_| String::new()));
            }
            marquee::keep(text, rest.first().map(String::as_str))
        }
        _ => Err("usage: g13map-editor [--version|--help]".into()),
    };
    finish(result);
}

fn finish(result: Result<String, String>) {
    match result {
        Ok(msg) if !msg.is_empty() => println!("{msg}"),
        Ok(_) => {}
        Err(e) => {
            eprintln!("g13map: {e}");
            std::process::exit(1);
        }
    }
}
