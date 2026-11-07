// SPDX-License-Identifier: GPL-3.0-or-later
//! What every live-display feed shares: another program leaves one line in a small file
//! saying what to show and for how long (`ttl SECS`), and the watcher reads the newest such
//! line it can trust. The families (alert, ship, combat, health, agent) keep their own
//! grammar in their own modules; the file protocol lives here once.
//!
//! The rule for several candidate files (a runtime copy and a state-dir copy, say): the newest
//! by write time wins; at the same time an off line beats a live one, then the path listed
//! first. A line that parses as nothing is skipped rather than counted, so a broken line left
//! by another program never clears a live feed; a line that was valid when written and has
//! since expired counts as off, so a stale newer file does suppress an older live one.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// A feed file longer than this is nobody's: the longest real line (a health line carrying
/// every extra) is under 300 bytes, and a reader must not load an arbitrary file.
pub(crate) const MAX_LINE: usize = 1024;

/// The `ttl SECS` word: live until `written + SECS`. Zero, negative, non-finite, unparsable
/// or expired rejects the line.
pub(crate) fn fresh(value: &str, written: SystemTime, now: SystemTime) -> Option<()> {
    let ttl = Duration::try_from_secs_f64(value.parse::<f64>().ok()?).ok()?;
    (!ttl.is_zero() && now.duration_since(written).unwrap_or_default() <= ttl).then_some(())
}

pub(crate) use crate::files::{modified, remove};

/// The file's text when it is at most `MAX_LINE` bytes of UTF-8.
pub(crate) fn read_line(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_LINE as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_LINE {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// What a line means now.
pub(crate) enum Outcome<S> {
    /// An off word, or a line that was valid when written and has expired: show nothing,
    /// and an older live file does not come back.
    Off,
    Live(S),
}

pub(crate) fn outcome<S>(
    text: &str,
    written: SystemTime,
    now: SystemTime,
    off_words: &[&str],
    parse: impl Fn(&str, SystemTime, SystemTime) -> Option<S>,
) -> Option<Outcome<S>> {
    if text
        .split_whitespace()
        .next()
        .is_some_and(|w| off_words.contains(&w))
    {
        return Some(Outcome::Off);
    }
    if let Some(state) = parse(text, written, now) {
        return Some(Outcome::Live(state));
    }
    parse(text, written, written).map(|_| Outcome::Off)
}

/// The newest trusted line among `paths` (the preferred path first, for ties).
pub(crate) fn newest<S>(
    paths: impl IntoIterator<Item = PathBuf>,
    off_words: &[&str],
    parse: impl Fn(&str, SystemTime, SystemTime) -> Option<S>,
) -> Option<S> {
    let now = SystemTime::now();
    paths
        .into_iter()
        .enumerate()
        .filter_map(|(priority, p)| {
            let written = modified(&p)?;
            let outcome = outcome(&read_line(&p)?, written, now, off_words, &parse)?;
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

/// The pair of places a feed may live: the session's runtime dir first, the state dir second.
pub(crate) fn pair(name: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = crate::daemon::runtime_file(name) {
        v.push(p);
    }
    v.push(crate::state_dir().join(name));
    v
}

/// One line to a feed file, atomically, ending in a newline.
pub(crate) fn write_line(path: &Path, line: &str) -> Result<(), String> {
    crate::files::atomic_write(path, format!("{}\n", line.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: SystemTime = SystemTime::UNIX_EPOCH;

    #[test]
    fn ttl_rules() {
        let w = T0 + Duration::from_secs(100);
        assert!(fresh("5", w, w + Duration::from_secs(5)).is_some());
        assert!(fresh("5", w, w + Duration::from_secs(6)).is_none());
        assert!(fresh("5", w, w).is_some());
        assert!(
            fresh("5", w, T0).is_some(),
            "a clock that went backwards reads as no time passed"
        );
        for bad in ["0", "-1", "inf", "NaN", "x", ""] {
            assert!(fresh(bad, w, w).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn outcome_three_ways() {
        let parse = |t: &str, w: SystemTime, n: SystemTime| -> Option<u32> {
            let mut words = t.split_whitespace();
            let v = words.next()?.parse().ok()?;
            fresh(words.nth(1)?, w, n)?;
            Some(v)
        };
        let w = T0 + Duration::from_secs(100);
        let soon = w + Duration::from_secs(1);
        let late = w + Duration::from_secs(50);
        assert!(matches!(
            outcome("7 ttl 5", w, soon, &["off"], parse),
            Some(Outcome::Live(7))
        ));
        assert!(
            matches!(
                outcome("7 ttl 5", w, late, &["off"], parse),
                Some(Outcome::Off)
            ),
            "expired = off"
        );
        assert!(matches!(
            outcome("off", w, soon, &["off"], parse),
            Some(Outcome::Off)
        ));
        assert!(matches!(
            outcome("wait ttl 5", w, soon, &["off", "wait"], parse),
            Some(Outcome::Off)
        ));
        assert!(
            outcome("garbage", w, soon, &["off"], parse).is_none(),
            "garbage is nothing, not off"
        );
        assert!(
            outcome("7 ttl 0", w, soon, &["off"], parse).is_none(),
            "never valid = garbage"
        );
    }

    #[test]
    fn newest_prefers_time_then_off_then_order() {
        let dir = std::env::temp_dir().join(format!("g13pad-feed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        // One clock for every write, so "same second" means the same instant.
        let base = SystemTime::now() - Duration::from_secs(1000);
        let at = |p: &Path, text: &str, secs: u64| {
            fs::write(p, text).unwrap();
            fs::File::options()
                .write(true)
                .open(p)
                .unwrap()
                .set_modified(base + Duration::from_secs(secs))
                .unwrap();
        };
        let parse = |t: &str, w: SystemTime, n: SystemTime| -> Option<u32> {
            let mut words = t.split_whitespace();
            let v = words.next()?.parse().ok()?;
            fresh(words.nth(1)?, w, n)?;
            Some(v)
        };
        let read = || newest([a.clone(), b.clone()], &["off"], parse);
        assert_eq!(read(), None);
        at(&a, "1 ttl 100000", 10);
        assert_eq!(read(), Some(1));
        at(&b, "2 ttl 100000", 20);
        assert_eq!(read(), Some(2), "newer wins");
        at(&b, "garbage", 30);
        assert_eq!(
            read(),
            Some(1),
            "garbage is skipped, the older live line stands"
        );
        at(&b, "off", 30);
        assert_eq!(read(), None, "off suppresses the older live line");
        at(&b, "2 ttl 1", 30);
        assert_eq!(read(), None, "expired counts as off");
        at(&a, "1 ttl 100000", 40);
        at(&b, "off", 40);
        assert_eq!(read(), None, "same time: off wins");
        at(&a, "1 ttl 100000", 50);
        at(&b, "2 ttl 100000", 50);
        assert_eq!(read(), Some(1), "same time, both live: the first path wins");
        at(&b, &format!("3 ttl 100000{}", " ".repeat(MAX_LINE)), 60);
        assert_eq!(read(), Some(1), "an oversize file is nobody's");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_line_uses_a_sibling_tmp_and_trims() {
        let dir = std::env::temp_dir().join(format!("g13pad-feedw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("g13map-g13.alert");
        write_line(&p, "  alert 10 X ttl 5  ").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "alert 10 X ttl 5\n");
        assert!(!dir.join("g13map-g13.alert.new").exists());
        assert!(
            !dir.join("g13map-g13.new").exists(),
            "the tmp keeps the file's own name"
        );
        remove(&p).unwrap();
        remove(&p).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
