// SPDX-License-Identifier: GPL-3.0-or-later
//! Feed responsibilities for Health Mode.
use super::*;

// ---- the feed ----

/// Where `g13map health` writes: the runtime file.
pub fn path() -> Result<PathBuf, String> {
    crate::daemon::runtime_file("health")
}

/// Where feeders may write: the runtime file, and `health` under the state directory
/// (`~/.local/state/g13map/`). A game in Steam's container has a private `/run/user`,
/// only the home directory is shared with the host (found 2026-10-07 with DRG), so a
/// mod inside it reaches the meter through the second one.
pub fn paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = path() {
        v.push(p);
    }
    v.push(crate::state_dir().join("health"));
    v
}

/// Makes the state directory, so a feeder in a container has somewhere to write, and
/// the tuning file with its defaults, commented, if there is none yet.
pub fn prepare() {
    let _ = fs::create_dir_all(crate::state_dir());
    let _ = fs::create_dir_all(crate::config_dir().join("meter.d"));
    let p = Tuning::path();
    if !p.exists() {
        let _ = fs::write(p, Tuning::default().to_text());
    }
}

/// `VALUE` as a whole number of tenths, 0–`max` (for `dash 2.5`).
pub(super) fn tenths(text: &str, max: f64) -> Option<u8> {
    let v = text.parse::<f64>().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some((v * 10.0).round().min(max * 10.0) as u8)
}

/// The percentage of `text` as `VALUE` or `VALUE/MAX`, rounded, 0–999.
pub(super) fn percent(text: &str) -> Option<u32> {
    let (v, max) = match text.split_once('/') {
        Some((v, m)) => (v.parse::<f64>().ok()?, m.parse::<f64>().ok()?),
        None => (text.parse::<f64>().ok()?, 100.0),
    };
    if !(v.is_finite() && max.is_finite()) || max <= 0.0 || v < 0.0 {
        return None;
    }
    Some((v / max * 100.0).round().clamp(0.0, 999.0) as u32)
}

/// A feed line, given when it was written: the state, or nothing for off, expired, or a
/// line that does not parse (a broken feeder is no feeder).
pub fn parse(text: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    if text.split_whitespace().next() == Some("travel") {
        return Travel::parse(text, written, now).map(State::Travel);
    }
    if text.split_whitespace().next() == Some("adv") {
        return Adventure::parse(text, written, now).map(State::Adventure);
    }
    if text.split_whitespace().next() == Some("fort") {
        return Fortress::parse(text, written, now).map(State::Fortress);
    }
    let mut words = text.split_whitespace();
    let mut state = match words.next()? {
        "wait" => State::Wait,
        "off" => return None,
        v => State::health(percent(v)?),
    };
    while let Some(key) = words.next() {
        let value = words.next()?;
        match key {
            "shield" => match &mut state {
                State::Health { shield, .. } => *shield = percent(value)?,
                _ => return None,
            },
            "helmet" => match (&mut state, value) {
                (State::Health { helmet, .. }, "on") => *helmet = true,
                (State::Health { helmet, .. }, "off") => *helmet = false,
                _ => return None,
            },
            "cap" | "rank" | "style" | "time" | "dash" | "rail" | "mana" | "defense" | "breath"
            | "battery" | "research" | "technology" | "attack" | "vehicle" | "pilot" => {
                let State::Health { extra, .. } = &mut state else {
                    return None;
                };
                match key {
                    "vehicle" => extra.vehicle = Label::parse(value)?,
                    "pilot" => extra.pilot = Some(percent(value)?),
                    "battery" => extra.battery = Some(percent(value)?.min(100) as u8),
                    "research" => extra.research = Some(percent(value)?.min(100) as u8),
                    "technology" => extra.technology = Label::parse(value)?,
                    "attack" => extra.attack = Some(value.parse::<u32>().ok()?.min(999)),
                    "cap" => extra.cap = Some(percent(value)?),
                    "rank" => extra.rank = Some(RANKS.iter().position(|r| *r == value)? as u8),
                    "style" => extra.style = Some(percent(value)?.min(100) as u8),
                    "time" => {
                        let s = value
                            .parse::<f64>()
                            .ok()
                            .filter(|s| s.is_finite() && *s >= 0.0)?;
                        extra.time = Some(s.min(359_999.0) as u32);
                    }
                    "dash" => extra.dash = Some(tenths(value, 3.0)?),
                    "mana" => extra.mana = Some(Resource::parse(value)?),
                    "breath" => extra.breath = Some(Resource::parse(value)?),
                    "defense" => extra.defense = Some(value.parse::<u32>().ok()?.min(999)),
                    _ => extra.rail = Some(percent(value)?.min(100) as u8),
                }
            }
            "ttl" => {
                let ttl = Duration::try_from_secs_f64(value.parse::<f64>().ok()?).ok()?;
                if ttl.is_zero() || now.duration_since(written).unwrap_or_default() > ttl {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(state)
}

/// The state of the most recently written feed file that has one.
pub fn read() -> Option<State> {
    let now = SystemTime::now();
    paths()
        .into_iter()
        .filter_map(|p| {
            let written = fs::metadata(&p).ok()?.modified().ok()?;
            let state = parse(&fs::read_to_string(&p).ok()?, written, now)?;
            Some((written, state))
        })
        .max_by_key(|(written, _)| *written)
        .map(|(_, state)| state)
}

/// Writes the feed (atomically: beside, then renamed) or, for `None`, removes every
/// feed file: no game.
pub fn write(state: Option<State>, ttl: Option<Duration>) -> Result<(), String> {
    let ttl = ttl.or_else(|| {
        matches!(
            state,
            Some(State::Fortress(_) | State::Adventure(_) | State::Travel(_))
        )
        .then(|| Duration::from_secs(6))
    });
    let p = path()?;
    let Some(state) = state else {
        for p in paths() {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        return Ok(());
    };
    let mut line = match state {
        State::Wait => "wait".to_string(),
        State::Fortress(fort) => fort.to_line(),
        State::Adventure(adv) => adv.to_line(),
        State::Travel(travel) => travel.to_line(),
        State::Health {
            pct,
            shield,
            helmet,
            extra,
        } => {
            let mut s = format!(
                "{pct} shield {shield}{}",
                if helmet { " helmet on" } else { "" }
            );
            if !extra.vehicle.as_str().is_empty() {
                s.push_str(&format!(" vehicle {}", extra.vehicle.as_str()));
            }
            if let Some(pilot) = extra.pilot {
                s.push_str(&format!(" pilot {pilot}"));
            }
            if let Some(c) = extra.cap {
                s.push_str(&format!(" cap {c}"));
            }
            if let Some(r) = extra.rank {
                s.push_str(&format!(" rank {}", RANKS[r as usize % RANKS.len()]));
            }
            if let Some(v) = extra.style {
                s.push_str(&format!(" style {v}"));
            }
            if let Some(t) = extra.time {
                s.push_str(&format!(" time {t}"));
            }
            if let Some(d) = extra.dash {
                s.push_str(&format!(" dash {}.{}", d / 10, d % 10));
            }
            if let Some(r) = extra.rail {
                s.push_str(&format!(" rail {r}"));
            }
            if let Some(m) = extra.mana {
                s.push_str(&format!(" mana {}/{}", m.current, m.maximum));
            }
            if let Some(d) = extra.defense {
                s.push_str(&format!(" defense {d}"));
            }
            if let Some(b) = extra.breath {
                s.push_str(&format!(" breath {}/{}", b.current, b.maximum));
            }
            if let Some(b) = extra.battery {
                s.push_str(&format!(" battery {b}"));
            }
            if let Some(r) = extra.research {
                s.push_str(&format!(" research {r}"));
            }
            if !extra.technology.as_str().is_empty() {
                s.push_str(&format!(" technology {}", extra.technology.as_str()));
            }
            if let Some(a) = extra.attack {
                s.push_str(&format!(" attack {a}"));
            }
            s
        }
    };
    if let Some(ttl) = ttl {
        line.push_str(&format!(" ttl {}", ttl.as_secs().max(1)));
    }
    line.push('\n');
    let tmp = p.with_extension("health.new");
    fs::write(&tmp, line)
        .and_then(|_| fs::rename(&tmp, &p))
        .map_err(|e| format!("{}: {e}", p.display()))
}
