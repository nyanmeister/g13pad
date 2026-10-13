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

enum Outcome {
    Off,
    Live(State),
}

/// Keep a valid shutdown/expiry in the timestamp ordering, but skip broken feeds.
fn outcome(text: &str, written: SystemTime, now: SystemTime) -> Option<Outcome> {
    if text.split_whitespace().next() == Some("off") {
        return Some(Outcome::Off);
    }
    if let Some(state) = parse(text, written, now) {
        return Some(Outcome::Live(state));
    }
    // The parsers use time only for TTL. A rejected line that was valid when
    // written has expired; revalidation also rejects garbage after a TTL field.
    parse(text, written, written).map(|_| Outcome::Off)
}

/// The newest valid feed decides, including off/expiry. At equal mtimes, off wins;
/// otherwise the runtime path takes precedence over the container's state path.
pub fn read() -> Option<State> {
    let now = SystemTime::now();
    paths()
        .into_iter()
        .enumerate()
        .filter_map(|(priority, p)| {
            let written = fs::metadata(&p).ok()?.modified().ok()?;
            let outcome = outcome(&fs::read_to_string(&p).ok()?, written, now)?;
            Some((written, priority, outcome))
        })
        .max_by_key(|(written, priority, outcome)| {
            (
                *written,
                matches!(outcome, Outcome::Off),
                std::cmp::Reverse(*priority),
            )
        })
        .and_then(|(_, _, outcome)| match outcome {
            Outcome::Off => None,
            Outcome::Live(state) => Some(state),
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;

    fn write_at(path: &Path, text: &str, written: SystemTime) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(written)
            .unwrap();
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), written);
    }

    #[test]
    fn newer_off_suppresses_older_live_feed() {
        let _sandbox = Sandbox::new("feed-newer-off");
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let t2 = t1 + Duration::from_secs(1);
        write_at(&crate::state_dir().join("health"), "100\n", t1);
        write_at(&path().unwrap(), "off\n", t2);

        // T2 explicitly ends the game; the no-TTL health left at T1 cannot revive it.
        assert_eq!(read(), None);
    }

    #[test]
    fn feed_ordering_handles_ties_garbage_expiry_and_missing_files() {
        let _sandbox = Sandbox::new("feed-ordering");
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let t2 = t1 + Duration::from_secs(1);
        let live = Some(State::health(100));
        // Expectations follow the file contract: valid newest wins, shutdown wins
        // a tie, broken data is skipped, and a missing file carries no instruction.
        let cases = [
            (
                "state off newer",
                Some(("100", t1)),
                Some(("off", t2)),
                None,
            ),
            (
                "runtime off older",
                Some(("off", t1)),
                Some(("100", t2)),
                live,
            ),
            (
                "state off older",
                Some(("100", t2)),
                Some(("off", t1)),
                live,
            ),
            (
                "runtime off tied",
                Some(("off", t1)),
                Some(("100", t1)),
                None,
            ),
            ("state off tied", Some(("100", t1)), Some(("off", t1)), None),
            (
                "live tie prefers runtime",
                Some(("100", t1)),
                Some(("70", t1)),
                live,
            ),
            (
                "newer state live",
                Some(("70", t1)),
                Some(("100", t2)),
                live,
            ),
            ("both off", Some(("off", t2)), Some(("off", t1)), None),
            ("both off tied", Some(("off", t1)), Some(("off", t1)), None),
            (
                "runtime garbage",
                Some(("garbage", t2)),
                Some(("100", t1)),
                live,
            ),
            (
                "state garbage",
                Some(("100", t1)),
                Some(("garbage", t2)),
                live,
            ),
            ("empty runtime", Some(("", t2)), Some(("100", t1)), live),
            (
                "runtime expired",
                Some(("70 ttl 1", t2)),
                Some(("100", t1)),
                None,
            ),
            (
                "state expired",
                Some(("100", t1)),
                Some(("70 ttl 1", t2)),
                None,
            ),
            (
                "expired wait",
                Some(("wait ttl 1", t2)),
                Some(("100", t1)),
                None,
            ),
            (
                "expiry tied",
                Some(("100", t1)),
                Some(("70 ttl 1", t1)),
                None,
            ),
            (
                "older expiry",
                Some(("70 ttl 1", t1)),
                Some(("100", t2)),
                live,
            ),
            (
                "garbage after expiry",
                Some(("70 ttl 1 bogus 7", t2)),
                Some(("100", t1)),
                live,
            ),
            (
                "invalid TTL",
                Some(("70 ttl bogus", t2)),
                Some(("100", t1)),
                live,
            ),
            (
                "zero TTL invalid",
                Some(("70 ttl 0", t2)),
                Some(("100", t1)),
                live,
            ),
            ("state missing", Some(("100", t1)), None, live),
            ("runtime missing", None, Some(("100", t1)), live),
            ("runtime off alone", Some(("off", t1)), None, None),
            ("state off alone", None, Some(("off", t1)), None),
            ("both missing", None, None, None),
        ];
        let runtime = path().unwrap();
        let state = crate::state_dir().join("health");
        for (name, runtime_record, state_record, expected) in cases {
            write(None, None).unwrap();
            if let Some((text, written)) = runtime_record {
                write_at(&runtime, text, written);
            }
            if let Some((text, written)) = state_record {
                write_at(&state, text, written);
            }
            assert_eq!(read(), expected, "{name}");
        }
    }

    #[test]
    fn expired_structured_feeds_suppress_older_health_but_broken_ones_do_not() {
        let _sandbox = Sandbox::new("feed-structured-expiry");
        let t1 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let t2 = t1 + Duration::from_secs(1);
        write_at(&crate::state_dir().join("health"), "100", t1);
        let runtime = path().unwrap();
        for line in [
            "fort 1 session 1 pop 84 care 6 inf 2 walk 3 hand 1 rags 17 worn 26 weather snow temp 9998 freeze freezing severity 2 alert 0 detail URIST_NEEDS_CRUTCH ttl 6",
            "adv 1 session 10 unit 281 name GUKI condition READY effort RESTED blood 100 wounds 0 walk 1 hand 1 allies 0/1/0 pets 0/23/0 severity 0 alert 0 phase 1 detail NO_ACTIVE_CONDITIONS ttl 6",
            "travel 1 session 20 unit 1309 name ELANA food 0 water 0 rest 0 activity WALKING allies 3 pets 4 phase 1 detail HEALTH_UNAVAILABLE_WHILE_TRAVELLING ttl 6",
        ] {
            write_at(&runtime, line, t2);
            assert_eq!(read(), None, "expired: {line}");
            // Even an expired record must validate all its required fields.
            write_at(&runtime, &line.replace("session", "bogus"), t2);
            assert_eq!(read(), Some(State::health(100)), "broken: {line}");
        }
    }
}
