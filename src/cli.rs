// SPDX-License-Identifier: GPL-3.0-or-later
//! CLI edits saved state; explicit `use`/`apply` are the hardware acceptance boundary.
use crate::{profile::StickMode, *};
use std::process::Command;

const HELP: &str = "g13map — Logitech G13 configuration

  g13map edit                         open the graphical editor
  g13map status                       connection, profile and configuration paths
  g13map profiles                     list saved profiles (* is active)
  g13map show [NAME]                   print a saved profile
  g13map use NAME                      apply and select a saved profile
  g13map apply                         reapply the selected profile
  g13map profile create NAME [FROM]    create empty or copy a saved profile
  g13map profile bind NAME KEY ACTION  save a physical KEY_* key/chord or raw action
  g13map profile unbind NAME KEY       remove a saved binding
  g13map profile rgb NAME R G B        save backlight colour (0–255)
  g13map profile leds NAME BITS        save M-key LED mask (0–15)
  g13map profile stick NAME MODE       analog, keys or current
  g13map profile controller NAME MAP   e.g. 'right r3 0 0 0' (stick/click/swap/invert-X/Y)
  g13map profile lcd NAME IMAGE        kept LCD image name, or none
  g13map profile health NAME MODE      health mode: off, feed (a mod or script),
                                       cs2 (the watcher's listener), log (a console log)
  g13map modes on|off|show             enable/disable/show M-Sum profile switching
  g13map modes set SUM NAME            assign a saved profile to sum 0–7
  g13map modes clear SUM               restore that sum's fallback
  g13map focus on|off|show             enable/disable/show i3 window rules
  g13map focus set CLASS NAME          assign a saved profile to a window class
  g13map focus clear CLASS             remove a window rule
  g13map panel lxqt|xfce|waybar|text    panel status (no arguments means LXQt)
  g13map import [FILE] [NAME]          import a driver bind file
  g13map marquee TEXT [NAME]           keep LCD text
  g13map health VALUE[/MAX] [shield S[/MAX]]  feed the health meter (needs the watcher)
  g13map health wait|off               game connected without health; no game
  g13map health demo                   a scripted pass through the meter's states
  g13map health cs2 [PORT]             Counter-Strike 2 Game State Integration feed
  g13map health cs2-config [PORT]      the cfg file the game needs for that
  g13map health source DIR [sp|mp]     Source engine game (Half-Life 2, CS:Source, ...):
                                       put the LCD module and page into its folder
                                       (singleplayer: with armour; multiplayer: health only)
  g13map health source DIR remove      take them out again
  g13map health source-res             that page file, for a game set up by hand
  g13map layout                       print active X11 layout's physical key labels
  g13map watch                        run the profile/LCD watcher
  g13map detach-pointer               isolate the G13 source pointer under X11
  g13map --version                    print version, without hardware or a display

Profile edits only save files. Use `g13map use NAME` to apply them.
M-Sum/window switching needs g13map-watch.service (also plays LCD animations).
Quote spaces/chords: g13map profile bind gaming G1 'KEY_LEFTCTRL+KEY_C'.
KEY_* names are physical codes; consult `g13map layout` for your layout's labels.
`g13map COMMAND --help` shows this help without invoking COMMAND.";

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("invalid {what}: '{text}'"))
}

fn control(name: &str) -> Result<(), String> {
    if profile::CONTROLS.contains(&name) {
        Ok(())
    } else {
        Err(format!("unknown G13 control '{name}'"))
    }
}

fn profile(args: &[&str]) -> Result<String, String> {
    let [op, name, rest @ ..] = args else {
        return Err("usage: g13map profile OP NAME [VALUES]; see --help".into());
    };
    validate_name(name)?;
    if *op == "create" {
        if rest.len() > 1 {
            return Err("usage: g13map profile create NAME [FROM]".into());
        }
        if profile_path(name).exists() {
            return Err(format!("profile '{name}' already exists"));
        }
        let p = match rest {
            [from] => load(from)?.0,
            _ => Profile::default(),
        };
        save(name, &p)?;
        return Ok(format!("created profile '{name}'"));
    }
    let (mut p, notes) = load(name)?;
    // Do not silently rewrite malformed settings during a narrow edit.
    if !notes.is_empty() {
        return Err(format!(
            "profile '{name}' needs repair before editing: {}",
            notes.join("; ")
        ));
    }
    match (*op, rest) {
        ("bind", [key, action @ ..]) if !action.is_empty() => {
            control(key)?;
            let action = action.join(" ");
            if action.chars().any(char::is_control) {
                return Err("binding must be a single line".into());
            }
            if action != profile::UNBOUND
                && action
                    .split_whitespace()
                    .any(|token| token.starts_with("KEY_") && profile::chord(token).is_none())
            {
                return Err("invalid key chord; use physical KEY_* names joined by +".into());
            }
            if action == profile::UNBOUND {
                p.binds.remove(*key);
            } else {
                p.binds.insert((*key).into(), action);
            }
        }
        ("unbind", [key]) => {
            control(key)?;
            p.binds.remove(*key);
        }
        ("rgb", [r, g, b]) => {
            p.rgb = Some([
                number(r, "red (0–255)")?,
                number(g, "green (0–255)")?,
                number(b, "blue (0–255)")?,
            ])
        }
        ("leds", [bits]) => {
            let bits = number::<u8>(bits, "LED mask (0–15)")?;
            if bits > 15 {
                return Err("LED mask must be 0–15".into());
            }
            p.leds = Some(bits);
        }
        ("stick", [mode]) => {
            p.stick = match *mode {
                "analog" => Some(StickMode::Analog),
                "keys" => Some(StickMode::Keys),
                "current" => None,
                _ => return Err("stick mode must be analog, keys or current".into()),
            }
        }
        ("controller", values) if !values.is_empty() => {
            p.gamepad = if values == ["default"] {
                None
            } else {
                Some(gamepad::Mapping::from_compact(&values.join(" "))?)
            };
        }
        ("health", [mode]) => {
            p.health = match *mode {
                "off" => None,
                word => Some(meter::Reader::parse(word).ok_or_else(|| {
                    format!("health mode must be off, feed, cs2 or log, not '{word}'")
                })?),
            };
        }
        ("lcd", [image]) => {
            if *image == "none" {
                p.lcd = None;
            } else {
                validate_name(image)?;
                if !lcd::path(image).is_file() && meter::selects(Some(image)).is_none() {
                    return Err(format!("kept LCD image '{image}' does not exist"));
                }
                p.lcd = Some((*image).into());
            }
        }
        _ => return Err(format!("invalid profile {op} arguments; see g13map --help")),
    }
    save(name, &p)?;
    Ok(format!(
        "saved profile '{name}'; apply with: g13map use '{name}'"
    ))
}

fn modes(args: &[&str]) -> Result<String, String> {
    let mut m = modes::Modes::load();
    match args {
        ["show"] => return Ok(m.to_text()),
        ["on"] => m.on = true,
        ["off"] => m.on = false,
        ["set", sum, name] => {
            let sum = number::<usize>(sum, "sum (0–7)")?;
            if sum > 7 {
                return Err("sum must be 0–7".into());
            }
            load(name)?;
            m.profiles[sum] = Some((*name).into());
        }
        ["clear", sum] => {
            let sum = number::<usize>(sum, "sum (0–7)")?;
            if sum > 7 {
                return Err("sum must be 0–7".into());
            }
            m.profiles[sum] = None;
        }
        _ => return Err("usage: g13map modes on|off|show|set SUM NAME|clear SUM".into()),
    }
    m.save()?;
    Ok("saved M-Sum settings; g13map-watch.service follows changes".into())
}

fn focus(args: &[&str]) -> Result<String, String> {
    let mut rules = focus::Rules::load();
    match args {
        ["show"] => return Ok(rules.to_text()),
        ["on"] => rules.on = true,
        ["off"] => rules.on = false,
        ["set", class, name] => {
            if class.is_empty()
                || class.trim() != *class
                || class.chars().any(char::is_control)
                || class.starts_with('#')
                || ["on", "off"].contains(class)
            {
                return Err("window class must be a nonempty single line other than on/off".into());
            }
            load(name)?;
            rules.set(class, Some(name));
        }
        ["clear", class] => rules.set(class, None),
        _ => return Err("usage: g13map focus on|off|show|set CLASS NAME|clear CLASS".into()),
    }
    rules.save()?;
    Ok("saved i3 rules; g13map-watch.service follows changes".into())
}

fn use_profile(name: &str) -> Result<String, String> {
    let (p, _) = load(name)?;
    let previous = load(&active_name()).ok().map(|(p, _)| p);
    let baseline = daemon_base();
    application::Plan {
        target: &p,
        previous: previous.as_ref(),
        baseline: baseline.as_ref(),
        routing: if modes::Modes::load().on {
            application::Routing::Routes
        } else {
            application::Routing::Profile
        },
    }
    .execute_complete()?;
    drop(lcd::show(p.lcd.as_deref(), false)?);
    set_active(name)?;
    Ok(format!("applied profile '{name}'"))
}

pub fn dispatch(args: &[String]) -> Result<String, String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    if matches!(a.as_slice(), ["help"] | ["--help"] | ["-h"])
        || (a.len() == 2 && ["--help", "-h"].contains(&a[1]))
    {
        return Ok(HELP.into());
    }
    match a.as_slice() {
        [] | ["panel", "lxqt"] => Ok(panel::lxqt()),
        ["panel", "xfce"] => panel::xfce(),
        ["panel", "waybar"] => Ok(panel::waybar()),
        ["panel", "text"] => Ok(panel::text()),
        ["status"] => Ok(format!(
            "{}\nprofiles: {}\ncommand pipe: {}",
            panel::text(),
            profiles_dir().display(),
            daemon::pipe_path().display()
        )),
        ["profiles"] => {
            let active = active_name();
            Ok(profile_names()
                .iter()
                .map(|n| format!("{} {n}", if *n == active { "*" } else { " " }))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        ["show"] => Ok(load(&active_name())?.0.to_text()),
        ["show", name] => Ok(load(name)?.0.to_text()),
        ["use", name] => use_profile(name),
        ["profile", rest @ ..] => profile(rest),
        ["modes", rest @ ..] => modes(rest),
        ["focus", rest @ ..] => focus(rest),
        ["edit"] => launch_editor(),
        ["apply"] => apply(),
        ["watch"] => modes::watch(),
        ["layout"] => serde_json::to_string_pretty(&keys::layout_map()).map_err(|e| e.to_string()),
        ["detach-pointer"] => session::detach_pointer().map(|d| d.to_string()),
        ["--version"] | ["version"] => Ok(format!("g13pad {} (g13map)", env!("CARGO_PKG_VERSION"))),
        ["marquee", text] => render_text(text, None),
        ["marquee", text, name] => {
            validate_name(name)?;
            render_text(text, Some(name))
        }
        ["health", rest @ ..] => health(rest),
        ["import"] => import(None, None),
        ["import", file] => import(Some(file), None),
        ["import", file, name] => import(Some(file), Some(name)),
        _ => Err("invalid command or arguments; see g13map --help".into()),
    }
}

/// `g13map health`: the meter's feed by hand, the demo, and the game adapters.
fn health(args: &[&str]) -> Result<String, String> {
    let port = |p: Option<&&str>| -> Result<u16, String> {
        p.map_or(Ok(meter::CS2_PORT), |p| number(p, "port"))
    };
    let feed = |state: Option<meter::State>| {
        meter::write(state, None)
            .map(|_| "fed the health meter; g13map-watch.service shows it".into())
    };
    match args {
        ["wait"] => feed(Some(meter::State::Wait)),
        ["off"] => feed(None),
        ["demo"] => meter::demo(),
        ["cs2", p @ ..] if p.len() <= 1 => meter::cs2(port(p.first())?),
        ["cs2-config", p @ ..] if p.len() <= 1 => Ok(meter::cs2_config(port(p.first())?)),
        ["source", dir] => meter::source_install(std::path::Path::new(dir), None),
        ["source", dir, "remove"] => meter::source_remove(std::path::Path::new(dir)),
        ["source", dir, kind] => match meter::SourceKind::parse(kind) {
            Some(k) => meter::source_install(std::path::Path::new(dir), Some(k)),
            None => Err(format!("a game's kind is sp or mp, not '{kind}'")),
        },
        ["source-res"] => Ok(meter::SOURCE_RES.to_string()),
        [value, ..] => {
            let line = args.join(" ");
            let now = std::time::SystemTime::now();
            match meter::parse(&line, now, now) {
                Some(state) => feed(Some(state)),
                None => Err(format!(
                    "invalid health line '{value} ...': VALUE[/MAX] [shield S[/MAX]] [helmet on|off] \
                     [cap C] [rank R] [style S] [time SECS] [dash D] [rail R] [ttl S]"
                )),
            }
        }
        _ => Err("usage: g13map health VALUE[/MAX] [WORD VALUE...] | wait | off | demo | cs2 [PORT] | cs2-config [PORT] | source DIR [sp|mp|remove] | source-res".into()),
    }
}

fn launch_editor() -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let editor = exe.with_file_name("g13map-editor");
    let error = Command::new(&editor).exec();
    Err(format!(
        "cannot open {}: {error}; install/build the editor component",
        editor.display()
    ))
}

pub(crate) fn editor_command() -> Result<Command, String> {
    let editor = env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("g13map-editor");
    Ok(Command::new(editor))
}

fn render_text(text: &str, name: Option<&str>) -> Result<String, String> {
    let mut command = editor_command()?;
    command.args(["--marquee", text]);
    if let Some(name) = name {
        command.arg(name);
    }
    let output = command
        .output()
        .map_err(|e| format!("LCD text needs the g13map-editor component: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;
    fn call(args: &[&str]) -> Result<String, String> {
        dispatch(&args.iter().map(|s| (*s).into()).collect::<Vec<_>>())
    }
    #[test]
    fn saved_edits_are_isolated_and_invalid_edits_leave_bytes_untouched() {
        let _box = Sandbox::new("cli");
        call(&["profile", "create", "ゲーム profile"]).unwrap();
        call(&[
            "profile",
            "bind",
            "ゲーム profile",
            "G1",
            "KEY_LEFTCTRL+KEY_C",
        ])
        .unwrap();
        call(&["profile", "rgb", "ゲーム profile", "1", "2", "3"]).unwrap();
        call(&["profile", "stick", "ゲーム profile", "analog"]).unwrap();
        let before = fs::read(profile_path("ゲーム profile")).unwrap();
        for args in [
            vec!["profile", "bind", "ゲーム profile", "G23", "KEY_D"],
            vec!["profile", "bind", "ゲーム profile", "G1", "KEY_WUT"],
            vec![
                "profile",
                "bind",
                "ゲーム profile",
                "G1",
                "KEY_D\nrgb 9 9 9",
            ],
            vec!["profile", "leds", "ゲーム profile", "16"],
            vec!["profile", "rgb", "ゲーム profile", "256", "2", "3"],
            vec!["profile", "create", "../escape"],
        ] {
            assert!(call(&args).is_err(), "{args:?}");
        }
        assert_eq!(before, fs::read(profile_path("ゲーム profile")).unwrap());
        assert!(!config_dir().join("active").exists());
        call(&["profile", "create", "copy", "ゲーム profile"]).unwrap();
        call(&["profile", "unbind", "copy", "G1"]).unwrap();
        assert!(load("copy").unwrap().0.binds.is_empty());
        call(&["profile", "bind", "copy", "G1", "KEY_A", "KEY_B"]).unwrap();
        assert_eq!(
            load("copy").unwrap().0.binds.get("G1").map(String::as_str),
            Some("KEY_A KEY_B")
        );
        call(&["profile", "bind", "copy", "G1", "KEY_RESERVED"]).unwrap();
        assert!(load("copy").unwrap().0.binds.is_empty());
        assert!(load("ゲーム profile").unwrap().0.binds.contains_key("G1"));
        call(&["modes", "set", "3", "copy"]).unwrap();
        call(&["focus", "set", "kitty", "copy"]).unwrap();
        assert_eq!(modes::Modes::load().profiles[3].as_deref(), Some("copy"));
        assert_eq!(focus::Rules::load().profile_for("kitty"), Some("copy"));
        assert!(call(&["edit", "--help"]).unwrap().contains("configuration"));
        assert!(call(&["apply", "extra"]).is_err());
    }
    #[test]
    fn failed_apply_keeps_the_selected_profile() {
        let _box = Sandbox::new("cli-use");
        save("old", &Profile::default()).unwrap();
        save("new", &Profile::default()).unwrap();
        set_active("old").unwrap();
        assert!(call(&["use", "new"]).is_err());
        assert_eq!(active_name(), "old");
    }
}
