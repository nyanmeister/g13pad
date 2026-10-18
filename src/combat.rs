// SPDX-License-Identifier: GPL-3.0-or-later
//! Combat performance, fed by a game observer. Shot attribution stays in the
//! observer; this screen knows only counters, score, time, health and a rank.
use crate::{
    draw::Draw,
    lcd::Bitmap,
    meter::{text, RANKS},
};
use std::{
    fs,
    io::Read,
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub hp: u32,
    pub shots: u64,
    pub hits: u64,
    pub style: u64,
    pub time_ms: u32,
    pub deaths: u32,
    pub rank: Option<u8>,
    pub known: bool,
}

impl State {
    pub fn accuracy(self) -> Option<u8> {
        (self.known && self.shots != 0).then(|| {
            ((self.hits as u128 * 100 + self.shots as u128 / 2) / self.shots as u128) as u8
        })
    }
    pub fn style_per_minute(self) -> Option<u64> {
        (self.time_ms != 0).then(|| {
            ((self.style as u128 * 60_000 + self.time_ms as u128 / 2) / self.time_ms as u128)
                .min(999_999_999) as u64
        })
    }
}

pub fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    let mut words = line.split_whitespace();
    if words.next()? != "combat" || words.next()? != "1" {
        return None;
    }
    let mut s = State {
        hp: 0,
        shots: 0,
        hits: 0,
        style: 0,
        time_ms: 0,
        deaths: 0,
        rank: None,
        known: false,
    };
    let keys = [
        "hp", "shots", "hits", "style", "time", "deaths", "rank", "known", "ttl",
    ];
    let mut seen = 0u16;
    while let Some(key) = words.next() {
        let index = keys.iter().position(|k| *k == key)?;
        if seen & (1 << index) != 0 {
            return None;
        }
        seen |= 1 << index;
        let value = words.next()?;
        match key {
            "hp" => s.hp = value.parse::<u32>().ok()?.min(999),
            "shots" => s.shots = value.parse().ok()?,
            "hits" => s.hits = value.parse().ok()?,
            "style" => s.style = value.parse().ok()?,
            "deaths" => s.deaths = value.parse().ok()?,
            "rank" => {
                s.rank = if value == "-" {
                    None
                } else {
                    Some(RANKS.iter().position(|r| *r == value)? as u8)
                }
            }
            "known" => {
                s.known = match value {
                    "0" => false,
                    "1" => true,
                    _ => return None,
                }
            }
            "time" => {
                let seconds = value.parse::<f64>().ok()?;
                if !seconds.is_finite() || !(0.0..=359_999.0).contains(&seconds) {
                    return None;
                }
                s.time_ms = (seconds * 1000.0).round() as u32;
            }
            "ttl" => {
                let ttl = Duration::try_from_secs_f64(value.parse().ok()?).ok()?;
                if ttl.is_zero() || now.duration_since(written).unwrap_or_default() > ttl {
                    return None;
                }
            }
            _ => unreachable!(),
        }
    }
    (seen == (1 << keys.len()) - 1 && s.hits <= s.shots).then_some(s)
}

pub fn read() -> Option<State> {
    let mut paths = vec![crate::state_dir().join("combat")];
    if let Ok(path) = crate::daemon::runtime_file("combat") {
        paths.push(path);
    }
    // A fresh wait/invalid record clears an older live record, never resurrects it.
    let (written, path) = paths
        .into_iter()
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(time, _)| *time)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(1025)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    parse(
        std::str::from_utf8(&bytes).ok()?,
        written,
        SystemTime::now(),
    )
}

pub fn frame(state: Option<State>) -> Bitmap {
    let mut bm = Bitmap::blank();
    text(&mut bm, 2, 1, "ACCURACY", 1);
    let accuracy = state
        .and_then(State::accuracy)
        .map_or_else(|| "--%".into(), |a| format!("{a}%"));
    text(&mut bm, 2, 12, &accuracy, 3);
    text(&mut bm, 88, 2, "STYLE/M", 1);
    let pace = state
        .and_then(State::style_per_minute)
        .map_or_else(|| "--".into(), |s| s.to_string());
    text(&mut bm, 88, 12, &pace, 1);
    let deaths = state.map_or_else(
        || "DEATHS --".into(),
        |s| {
            if s.deaths < 1000 {
                format!("DEATHS {}", s.deaths)
            } else {
                format!("D {}", s.deaths)
            }
        },
    );
    text(&mut bm, 88, 23, &deaths, 1);
    let hp = state.map_or_else(|| "HP --".into(), |s| format!("HP {}", s.hp));
    text(&mut bm, 2, 35, &hp, 1);
    if let Some(s) = state {
        let seconds = s.time_ms / 1000;
        text(
            &mut bm,
            75,
            35,
            &format!(
                "{}  {:02}:{:02}",
                s.rank
                    .and_then(|r| RANKS.get(r as usize))
                    .copied()
                    .unwrap_or("--"),
                seconds / 60,
                seconds % 60
            ),
            1,
        );
    }
    bm.line(77, 1, 77, 29);
    bm
}

pub(crate) fn paint(tuning: &crate::meter::Tuning, rest: [u8; 3]) -> (Bitmap, [u8; 3]) {
    let state = read();
    let colour = state.map_or(rest, |s| {
        if s.hp == 0 {
            [255, 0, 0]
        } else {
            tuning.rgb_at(s.hp)
        }
    });
    (frame(state), colour)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel_feed::Live;
    use std::thread;
    const EXAMPLE: &str =
        "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 3";
    fn parsed(line: &str) -> Option<State> {
        parse(line, SystemTime::UNIX_EPOCH, SystemTime::UNIX_EPOCH)
    }
    #[test]
    fn independently_counted_shots_and_level_pace() {
        let state = parsed(EXAMPLE).unwrap();
        // 43 / 50 = 86%; 8928 points / (124 seconds / 60) = 4320 points/min.
        assert_eq!(state.accuracy(), Some(86));
        assert_eq!(state.style_per_minute(), Some(4320));
        assert_eq!(state.deaths, 2);
        assert_eq!(state.time_ms, 124_000);
        assert_eq!(
            parsed(&EXAMPLE.replace("shots 50 hits 43", "shots 0 hits 0"))
                .unwrap()
                .accuracy(),
            None
        );
        assert_eq!(
            parsed(&EXAMPLE.replace("known 1", "known 0"))
                .unwrap()
                .accuracy(),
            None
        );
        assert_eq!(
            parsed(&EXAMPLE.replace("time 124.000", "time 0"))
                .unwrap()
                .style_per_minute(),
            None
        );
        assert_eq!(
            parsed(&EXAMPLE.replace("shots 50 hits 43", "shots 8 hits 3"))
                .unwrap()
                .accuracy(),
            Some(38)
        );
    }
    #[test]
    fn rejects_incoherent_incomplete_duplicate_and_expired_records() {
        for invalid in [
            EXAMPLE.replace("hits 43", "hits 51"),
            EXAMPLE.replace("time 124.000", "time NaN"),
            EXAMPLE.replace("time 124.000", "time -1"),
            EXAMPLE.replace("rank SS", "rank Q"),
            EXAMPLE.replace("known 1", "known 2"),
            EXAMPLE.replace("shots 50", "shots 18446744073709551616"),
            EXAMPLE.replace("ttl 3", "ttl 0"),
            EXAMPLE.replace("ttl 3", "ttl inf"),
            EXAMPLE.replace("deaths 2 ", ""),
            format!("{EXAMPLE} hits 0"),
        ] {
            assert!(parsed(&invalid).is_none(), "{invalid}");
        }
        assert!(parse(
            EXAMPLE,
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::from_millis(3001)
        )
        .is_none());
        assert!(parse(
            EXAMPLE,
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::from_secs(3)
        )
        .is_some());
    }
    #[test]
    fn randomized_valid_records_and_single_field_mutations() {
        let mut entropy = [0u8; 8];
        fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut entropy)
            .unwrap();
        let mut seed = u64::from_ne_bytes(entropy) | 1;
        eprintln!("combat parser seed {seed}");
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..2000 {
            let shots = next() % 999_999 + 1;
            let hits = next() % (shots + 1);
            let line = format!("combat 1\nhits {hits} hp 100 shots {shots} style 1000 time 60 deaths 0 rank D known 1 ttl 3");
            let state = parsed(&line).unwrap();
            assert_eq!(
                (state.shots, state.hits, state.style_per_minute()),
                (shots, hits, Some(1000))
            );
            assert!(state.accuracy().unwrap() <= 100);
            assert!(parsed(&format!("{line} shots {shots}")).is_none());
            assert!(
                parsed(&line.replace(&format!("hits {hits}"), &format!("hits {}", shots + 1)))
                    .is_none()
            );
        }
    }
    #[test]
    fn freshest_wait_clears_stale_other_path_and_large_input_is_bounded() {
        let _sandbox = crate::test_support::Sandbox::new("combat-feed");
        let state = crate::state_dir().join("combat");
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, EXAMPLE).unwrap();
        assert!(read().is_some());
        thread::sleep(Duration::from_millis(20));
        let runtime = crate::daemon::runtime_file("combat").unwrap();
        fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        fs::write(&runtime, "wait ttl 3").unwrap();
        assert!(read().is_none());
        fs::write(&runtime, "x".repeat(1025)).unwrap();
        assert!(read().is_none());
    }
    #[test]
    fn live_thread_writes_once_for_static_pixels_and_stops() {
        let _sandbox = crate::test_support::Sandbox::new("combat-live");
        let pipe = crate::test_support::PanelPipe::new();
        let path = crate::state_dir().join("combat");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, EXAMPLE).unwrap();
        let live = Live::start([0, 0, 0], "test", paint);
        let mut bytes = pipe.read();
        // RGB and LCD may arrive in separate FIFO reads.
        if !bytes
            .windows(crate::lcd::BYTES)
            .any(|w| w == frame(parsed(EXAMPLE)).0)
        {
            bytes.extend(pipe.read());
        }
        assert!(bytes
            .windows(crate::lcd::BYTES)
            .any(|w| w == frame(parsed(EXAMPLE)).0));
        pipe.expect_quiet();
        fs::write(&path, EXAMPLE.replace("124.000", "124.001")).unwrap();
        pipe.expect_quiet(); // Subsecond telemetry changed, but no visible pixels did.
        drop(live);
        let _ = pipe.read(); // Resting-colour restoration.
        pipe.expect_quiet();
    }
}
