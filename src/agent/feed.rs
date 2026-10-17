// SPDX-License-Identifier: GPL-3.0-or-later
//! The agent feed: one line in one file, the same shape as the health feed (two paths, the
//! newest valid wins, `off` ends it, a broken line is no feeder). See the health
//! feed-contract note; this is its sibling for the agent screen.
use super::*;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// The directory of per-terminal feeds: one file per agent, named by its terminal's X11
/// window id (`$WINDOWID`), so two agents on two terminals never collide. In the runtime
/// dir (tmpfs, cleared on reboot); terminals run on the host, so there is no Proton-container
/// private `/run/user` to reach around like the health feed has.
pub fn dir() -> Result<PathBuf, String> {
    let p = crate::daemon::runtime_file("agents")?;
    fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

/// One terminal's feed file.
pub fn file(win: u64) -> Result<PathBuf, String> {
    Ok(dir()?.join(win.to_string()))
}

/// A feed instant: unix-epoch milliseconds.
fn epoch(value: &str) -> Option<SystemTime> {
    Some(SystemTime::UNIX_EPOCH + Duration::from_millis(value.parse::<u64>().ok()?))
}

/// A feed duration in milliseconds, bounded to a day's worth of turn so a garbage value
/// cannot make the stopwatch meaningless.
fn millis(value: &str) -> Option<Duration> {
    let ms = value.parse::<u64>().ok()?;
    (ms <= 86_400_000).then(|| Duration::from_millis(ms))
}

/// One feed line, given when it was written: the state, or nothing for `off`, expiry, or a
/// line that does not parse. A turn must say when it `started`; every other field is
/// optional. An unknown word rejects the whole line — a broken feeder is no feeder.
pub fn parse(text: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    enum Base {
        Turn(Activity),
        Ask(Attention),
        Ready,
        Denied,
        Idle,
        Waiting,
    }
    let mut words = text.split_whitespace();
    let base = match words.next()? {
        "off" => return None,
        "ask" => Base::Ask(Attention::Permission),
        "input" => Base::Ask(Attention::Question { asynchronous: true }),
        "question" => Base::Ask(Attention::Question {
            asynchronous: false,
        }),
        "review" => Base::Ask(Attention::Review),
        "ready" => Base::Ready,
        "denied" => Base::Denied,
        "idle" => Base::Idle,
        "wait" => Base::Waiting,
        "turn" => Base::Turn(Activity::parse(words.next()?)),
        _ => return None,
    };
    let mut trailer = Trailer::default();
    let mut started: Option<SystemTime> = None;
    while let Some(key) = words.next() {
        let value = words.next()?;
        match key {
            "started" => started = Some(epoch(value)?),
            "agents" => trailer.agents = value.parse::<u32>().ok()?.min(MAX_AGENTS),
            "last" => trailer.last = Some(millis(value)?),
            "fail" => trailer.fail_at = Some(epoch(value)?),
            "deny" => trailer.deny_at = Some(epoch(value)?),
            "ttl" => {
                let ttl = Duration::try_from_secs_f64(value.parse::<f64>().ok()?).ok()?;
                if ttl.is_zero() || now.duration_since(written).unwrap_or_default() > ttl {
                    return None;
                }
            }
            _ => return None,
        }
    }
    match base {
        Base::Turn(activity) => Some(State::Turn {
            activity,
            started: started?, // a turn without a start has no stopwatch: reject it
            trailer,
        }),
        Base::Ask(attention) => Some(State::Ask {
            attention,
            started,
            trailer,
        }),
        Base::Ready => Some(State::Ready { trailer }),
        Base::Denied => Some(State::Denied { trailer }),
        Base::Idle => Some(State::Idle { trailer }),
        Base::Waiting => Some(State::Waiting { trailer }),
    }
}

enum Outcome {
    Off,
    Live(State),
}

/// Keep a valid shutdown/expiry in the timestamp ordering, but skip broken feeds. Mirrors
/// the health feed: a line that was valid when written but has since expired counts as a
/// clean `Off`, so it can still suppress an older live line; genuine garbage cannot.
fn outcome(text: &str, written: SystemTime, now: SystemTime) -> Option<Outcome> {
    if text.split_whitespace().next() == Some("off") {
        return Some(Outcome::Off);
    }
    if let Some(state) = parse(text, written, now) {
        return Some(Outcome::Live(state));
    }
    parse(text, written, written).map(|_| Outcome::Off)
}

/// Every terminal with a live feed now: (window id, mtime, state), newest last by nothing
/// in particular — the caller sorts. Expired, `off` and broken files are dropped; a stale
/// file whose terminal has gone is just an expired `ttl` the watcher skips (and the runtime
/// dir clears on reboot). This is the selection surface: the watcher picks by focus.
pub fn read_all() -> Vec<(u64, SystemTime, State)> {
    let now = SystemTime::now();
    let Ok(d) = dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&d) else {
        return Vec::new();
    };
    let mut live = Vec::new();
    for entry in entries.flatten() {
        let Some(win) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue; // not a `<winid>` feed file (a temp `.new`, say)
        };
        let Ok(written) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Some(Outcome::Live(state)) = outcome(&text, written, now) {
            live.push((win, written, state));
        }
    }
    live
}

/// One terminal's live state, if its feed is fresh. The watcher's focus-aware read.
pub fn read_for(win: u64) -> Option<State> {
    let now = SystemTime::now();
    let p = file(win).ok()?;
    let written = fs::metadata(&p).ok()?.modified().ok()?;
    match outcome(&fs::read_to_string(&p).ok()?, written, now)? {
        Outcome::Live(state) => Some(state),
        Outcome::Off => None,
    }
}

/// The newest live feed across all terminals. A focus-agnostic fallback the watcher uses
/// until it has a focused window to key on, and the single-agent case.
pub fn read() -> Option<State> {
    read_all()
        .into_iter()
        .max_by_key(|(_, written, _)| *written)
        .map(|(_, _, state)| state)
}

/// Focus-aware selection for the watcher (the "latest agent", asked 2026-10-15). A
/// focused window that has a live feed becomes `latest`; focusing a window with no feed (a
/// browser, a plain terminal) leaves the pointer where it was, so the display follows the
/// last agent terminal touched, surviving a non-terminal focus. Returns the latest agent's
/// live state, else — if its feed has gone stale — the newest live feed anywhere, else None.
pub fn select(focused: Option<u64>, latest: &mut Option<u64>) -> Option<State> {
    if let Some(w) = focused {
        if read_for(w).is_some() {
            *latest = Some(w);
        }
    }
    (*latest).and_then(read_for).or_else(read)
}

/// Writes one terminal's feed (atomically), or removes it for `None`: that agent is gone. A
/// feeder sends a `ttl` so the page ends by itself when the agent dies without an `off`.
pub fn write(win: u64, state: Option<State>, ttl: Option<Duration>) -> Result<(), String> {
    let p = file(win)?;
    let Some(state) = state else {
        return match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", p.display())),
        };
    };
    let mut line = state.to_line();
    if let Some(ttl) = ttl {
        line.push_str(&format!(" ttl {}", ttl.as_secs().max(1)));
    }
    line.push('\n');
    let tmp = p.with_extension("new");
    fs::write(&tmp, line)
        .and_then(|_| fs::rename(&tmp, &p))
        .map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;
    use std::path::Path;

    fn at(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }
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
    fn a_turn_parses_every_field_by_hand() {
        // Hand-derived: a turn editing, started at epoch 8.345 s, two agents out, the last
        // turn took 178.9 s, a failure 0.4 s ago, no denial. now == written so no expiry.
        let now = at(10_000);
        let s = parse(
            "turn RUNNING_BASH started 8345 agents 2 last 178900 fail 9600 ttl 30",
            now,
            now,
        )
        .unwrap();
        let State::Turn {
            activity,
            started,
            trailer,
        } = s
        else {
            panic!("expected a turn");
        };
        assert_eq!(activity.as_str(), "RUNNING BASH"); // underscore -> space, upper
        assert_eq!(started, at(8_345));
        assert_eq!(trailer.agents, 2);
        assert_eq!(trailer.last, Some(Duration::from_millis(178_900)));
        assert_eq!(trailer.fail_at, Some(at(9_600)));
        assert_eq!(trailer.deny_at, None);
    }

    #[test]
    fn the_non_turn_phases_and_their_rejections() {
        let now = at(1);
        assert!(matches!(
            parse("ask agents 1", now, now),
            Some(State::Ask { .. })
        ));
        assert!(matches!(
            parse("ready last 5000", now, now),
            Some(State::Ready { .. })
        ));
        assert!(matches!(parse("idle", now, now), Some(State::Idle { .. })));
        // A garbage activity token degrades to THINKING, it does not reject the line.
        let State::Turn { activity, .. } = parse("turn ***!!! started 1", now, now).unwrap() else {
            panic!("expected a turn");
        };
        assert_eq!(activity.as_str(), "THINKING");
        // Rejections (a broken feeder is no feeder):
        assert!(parse("turn EDITING agents 2", now, now).is_none()); // a turn with no start
        assert!(parse("turn EDITING started nope", now, now).is_none()); // non-numeric start
        assert!(parse("ready last 999999999", now, now).is_none()); // duration past a day
        assert!(parse("idle bogus 7", now, now).is_none()); // unknown word
        assert!(parse("ask agents", now, now).is_none()); // key with no value
        assert!(parse("off", now, now).is_none()); // off is an end, not a state
        assert!(parse("", now, now).is_none()); // empty
    }

    #[test]
    fn to_line_round_trips() {
        let now = at(0);
        for line in [
            "turn EDITING started 8345",
            "turn WEB_SEARCH started 8345 agents 3 last 178900 fail 9600 deny 9700",
            "ask agents 1",
            "ready last 5000",
            "idle",
        ] {
            let state = parse(line, now, now).expect(line);
            assert_eq!(state.to_line(), line, "{line}");
        }
    }

    #[test]
    fn per_window_feeds_select_expire_and_round_trip() {
        let _sandbox = Sandbox::new("agent-feed-windows");
        let old = at(1_000_000); // long ago: a ttl'd feed written then has expired
        let newer = at(1_001_000);
        // Two terminals, two agents, each its own file keyed by window id.
        write_at(&file(100).unwrap(), "turn EDITING started 8345", old);
        write_at(&file(200).unwrap(), "ready", newer);
        // read_for keys on the window: 100 is editing, 200 is ready, 999 has nothing.
        assert!(matches!(read_for(100), Some(State::Turn { .. })));
        assert!(matches!(read_for(200), Some(State::Ready { .. })));
        assert_eq!(read_for(999), None);
        // read() (the focus-agnostic fallback) takes the newest-written: 200.
        assert!(matches!(read(), Some(State::Ready { .. })));
        // read_all lists exactly the live terminals.
        let all = read_all();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(w, _, _)| *w == 100) && all.iter().any(|(w, _, _)| *w == 200));
        // Expired (ttl 1 s, written in 1970), `off`, and a broken line are all not live.
        write_at(&file(300).unwrap(), "idle ttl 1", old);
        write_at(&file(400).unwrap(), "off", newer);
        write_at(&file(500).unwrap(), "garbage", newer);
        assert_eq!(read_for(300), None);
        assert_eq!(read_for(400), None);
        assert_eq!(read_for(500), None);
        assert_eq!(read_all().len(), 2); // still just 100 and 200
                                         // A non-`<winid>` file name in the dir is ignored, never parsed as a feed.
        write_at(&dir().unwrap().join("notawindow"), "ready", newer);
        assert_eq!(read_all().len(), 2);
        // write(win, None) removes that terminal's file.
        write(100, None, None).unwrap();
        assert_eq!(read_for(100), None);
        assert!(!file(100).unwrap().exists());
        // write + read_for round-trips a live state (ttl keeping it fresh).
        write(
            700,
            Some(State::Turn {
                activity: Activity::parse("EDITING"),
                started: at(8_345),
                trailer: Trailer {
                    agents: 2,
                    ..Default::default()
                },
            }),
            Some(Duration::from_secs(30)),
        )
        .unwrap();
        let State::Turn { trailer, .. } = read_for(700).unwrap() else {
            panic!("expected a turn");
        };
        assert_eq!(trailer.agents, 2);
    }

    #[test]
    fn select_follows_focus_sticks_and_falls_back() {
        let _sandbox = Sandbox::new("agent-select");
        let now = SystemTime::now(); // fresh feeds: no ttl, never expire
        write_at(&file(10).unwrap(), "turn EDITING started 8345", now);
        write_at(&file(20).unwrap(), "ready", now);
        let mut latest = None;
        // Focus terminal 10 (has a feed): it becomes latest, its turn shows.
        assert!(matches!(
            select(Some(10), &mut latest),
            Some(State::Turn { .. })
        ));
        assert_eq!(latest, Some(10));
        // Focus a browser (window 999, no feed): the pointer HOLDS, 10 still shows.
        assert!(matches!(
            select(Some(999), &mut latest),
            Some(State::Turn { .. })
        ));
        assert_eq!(latest, Some(10));
        // Focus terminal 20: it becomes latest, its ready shows.
        assert!(matches!(
            select(Some(20), &mut latest),
            Some(State::Ready { .. })
        ));
        assert_eq!(latest, Some(20));
        // latest's session ends (its feed removed); focus a non-agent window: fall back to
        // the newest live feed anywhere (10's turn), the pointer not advanced to the browser.
        write(20, None, None).unwrap();
        assert!(matches!(
            select(Some(999), &mut latest),
            Some(State::Turn { .. })
        ));
        // No live feeds and no focusable agent: nothing to show.
        write(10, None, None).unwrap();
        let mut none = None;
        assert_eq!(select(None, &mut none), None);
    }
}
