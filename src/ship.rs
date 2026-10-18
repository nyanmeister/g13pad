// SPDX-License-Identifier: GPL-3.0-or-later
//! A ship's hull, optional shields and energy. No heartbeat, including while waiting.
use crate::{
    draw::Draw,
    lcd::Bitmap,
    meter::{gauge, parts::Pie, text},
};
use std::{
    fs,
    io::Read,
    time::{Duration, SystemTime},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub hull: u32,
    pub shield: Option<u32>,
    pub energy: Option<u32>,
    /// Whole hull percentage currently repairable from carried supplies.
    pub repair: Option<u8>,
    /// Game seconds until shields can begin recharging.
    pub shield_delay: Option<u16>,
    /// Estimated seconds: negative until empty, positive until full.
    pub energy_time: Option<i16>,
}
pub fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    let mut words = line.split_whitespace();
    if words.next()? != "ship" || words.next()? != "1" {
        return None;
    }
    let mut state = State::default();
    let mut seen = 0;
    while let Some(key) = words.next() {
        let bit = match key {
            "hull" => 1,
            "shield" => 2,
            "energy" => 4,
            "ttl" => 8,
            "repair" => 16,
            "shield_delay" => 32,
            "energy_time" => 64,
            _ => return None,
        };
        if seen & bit != 0 {
            return None;
        }
        seen |= bit;
        let value = words.next()?;
        let percentage = || value.parse::<u32>().ok().filter(|n| *n <= 999);
        match key {
            "hull" => state.hull = percentage()?,
            "shield" => {
                state.shield = if value == "none" {
                    None
                } else {
                    Some(percentage()?)
                }
            }
            "energy" => {
                state.energy = if value == "none" {
                    None
                } else {
                    Some(percentage()?)
                }
            }
            "repair" => state.repair = Some(value.parse::<u8>().ok().filter(|n| *n <= 100)?),
            "shield_delay" => {
                state.shield_delay = Some(value.parse::<u16>().ok().filter(|n| *n <= 9999)?)
            }
            "energy_time" => {
                state.energy_time = Some(
                    value
                        .parse::<i16>()
                        .ok()
                        .filter(|n| (-9999..=9999).contains(n))?,
                )
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
    (seen & 15 == 15
        && (state.shield.is_some() || state.shield_delay.is_none())
        && (state.energy.is_some() || state.energy_time.is_none()))
    .then_some(state)
}
pub fn read() -> Option<State> {
    let configured = std::env::var_os("G13MAP_SHIP_FILE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            fs::read_to_string(crate::config_dir().join("ship-feed"))
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|s| std::path::PathBuf::from(s.trim()))
        });
    let paths = if let Some(path) = configured {
        vec![path]
    } else {
        let mut paths = vec![crate::state_dir().join("ship")];
        if let Ok(path) = crate::daemon::runtime_file("ship") {
            paths.push(path);
        }
        paths
    };
    let (written, path) = paths
        .into_iter()
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(time, _)| *time)?;
    let mut line = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(513)
        .read_to_end(&mut line)
        .ok()?;
    if line.len() > 512 {
        return None;
    }
    parse(std::str::from_utf8(&line).ok()?, written, SystemTime::now())
}
const ENERGY: Pie = Pie {
    cx: 26,
    cy: 21,
    disc: 10,
    ring_in: 12,
    ring_out: 13,
    segments: 12,
};
const BOLT: [&str; 11] = [
    "....###", "...###.", "..###..", ".###...", "######.", "######.", "...##..", "..##...",
    ".##....", "##.....", "#......",
];
const HOURGLASS: [&str; 7] = [
    "#######", ".#...#.", "..#.#..", "...#...", "..#.#..", ".#...#.", "#######",
];
fn width(text: &str) -> i32 {
    text.len() as i32 * 6 - 1
}
fn countdown(seconds: u16) -> String {
    if seconds < 1000 {
        format!("{seconds}s")
    } else {
        format!("{}m", seconds.div_ceil(60))
    }
}
pub fn frame(state: Option<State>) -> Bitmap {
    let mut bm = Bitmap::blank();
    text(&mut bm, 9, 0, "ENERGY", 1);
    ENERGY.draw(
        &mut bm,
        state.and_then(|s| s.energy).unwrap_or(0) as f32 / 100.0,
    );
    // Inverse lightning bolt: dark in the filled sector, lit in the empty sector.
    for (dy, row) in BOLT.iter().enumerate() {
        for (dx, c) in row.bytes().enumerate() {
            if c == b'#' {
                let (x, y) = (23 + dx, 16 + dy);
                bm.set(x, y, !bm.get(x, y));
            }
        }
    }
    let percent = |v: Option<u32>| v.map_or_else(|| "--".into(), |n| format!("{n}%"));
    let mut energy = percent(state.and_then(|s| s.energy));
    if let Some(time) = state.and_then(|s| s.energy_time) {
        let time = format!(
            "{}{}",
            if time < 0 { "-" } else { "+" },
            countdown(time.unsigned_abs())
        );
        let separator = if energy.len() + time.len() < 9 {
            " "
        } else {
            ""
        };
        energy.push_str(separator);
        energy.push_str(&time);
    }
    text(&mut bm, 26 - width(&energy) / 2, 36, &energy, 1);
    // Keep whole values legible even at the feed's bounds. Removing the label's
    // word gap is enough for the widest pair; the two readouts stay separated.
    let paired = |bm: &mut Bitmap, y, label, value: String, extra: Option<String>, icon| {
        let right = extra
            .as_ref()
            .map_or(0, |s| width(s) + if icon { 12 } else { 0 });
        let gap = if width(&format!("{label} {value}")) + right + 4 <= 104 {
            " "
        } else {
            ""
        };
        text(bm, 54, y, &format!("{label}{gap}{value}"), 1);
        if let Some(extra) = extra {
            let x = (if icon { 156 } else { 157 }) - width(&extra);
            if icon {
                bm.sprite(x - 12, y, &HOURGLASS);
            }
            text(bm, x, y, &extra, 1);
        }
    };
    paired(
        &mut bm,
        1,
        "HULL",
        percent(state.map(|s| s.hull)),
        state.and_then(|s| s.repair).map(|n| format!("FIX +{n}%")),
        false,
    );
    gauge(&mut bm, 54, 11, 104, 8, state.map_or(0, |s| s.hull));
    if state.is_some_and(|s| s.shield.is_none()) {
        text(&mut bm, 54, 24, "NO SHIELD", 1);
    } else {
        let delay = state.and_then(|s| s.shield_delay);
        paired(
            &mut bm,
            24,
            "SHIELD",
            percent(state.and_then(|s| s.shield)),
            delay.map(|n| if n == 0 { "READY".into() } else { countdown(n) }),
            delay.is_some_and(|n| n > 0),
        );
        gauge(
            &mut bm,
            54,
            34,
            104,
            8,
            state.and_then(|s| s.shield).unwrap_or(0),
        );
    }
    bm
}
pub(crate) fn paint(tuning: &crate::meter::Tuning, rest: [u8; 3]) -> (Bitmap, [u8; 3]) {
    let state = read();
    (frame(state), state.map_or(rest, |s| tuning.rgb_at(s.hull)))
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLE: &str = "ship 1 hull 74 shield 38 energy 78 ttl 3";
    fn parsed(line: &str) -> Option<State> {
        parse(line, SystemTime::UNIX_EPOCH, SystemTime::UNIX_EPOCH)
    }
    #[test]
    #[ignore] // Explicit review artifact export; never writes into the user's feed.
    fn export_ship_frames() {
        let Some(dir) = std::env::var_os("G13MAP_SHIP_DUMP") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let base = State {
            hull: 74,
            shield: Some(38),
            energy: Some(78),
            repair: Some(12),
            shield_delay: Some(8),
            energy_time: Some(-6),
        };
        for (name, state) in [
            ("final-shield", Some(base)),
            (
                "final-unshielded",
                Some(State {
                    hull: 56,
                    shield: None,
                    energy: Some(93),
                    repair: Some(43),
                    ..Default::default()
                }),
            ),
            (
                "final-ready",
                Some(State {
                    shield_delay: Some(0),
                    energy_time: Some(12),
                    ..base
                }),
            ),
            (
                "final-depleted",
                Some(State {
                    hull: 20,
                    shield: Some(0),
                    energy: Some(100),
                    shield_delay: Some(18),
                    energy_time: None,
                    ..base
                }),
            ),
            (
                "final-wide",
                Some(State {
                    hull: 999,
                    shield: Some(999),
                    energy: Some(999),
                    repair: Some(100),
                    shield_delay: Some(999),
                    energy_time: Some(-999),
                }),
            ),
            (
                "final-long",
                Some(State {
                    shield_delay: Some(9999),
                    energy_time: Some(9999),
                    ..base
                }),
            ),
            ("final-wait", None),
        ] {
            let bm = frame(state);
            let mut pbm = String::from("P1\n160 43\n");
            for y in 0..43 {
                for x in 0..160 {
                    pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
                }
                pbm.push('\n');
            }
            fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
        }
    }
    #[test]
    fn shield_absence_is_different_from_depletion_and_expiry() {
        assert_eq!(
            parsed(EXAMPLE),
            Some(State {
                hull: 74,
                shield: Some(38),
                energy: Some(78),
                ..Default::default()
            })
        );
        let missing = parsed("ship 1 hull 56 shield none energy 93 ttl 3").unwrap();
        let empty = parsed("ship 1 hull 56 shield 0 energy 93 ttl 3").unwrap();
        assert!(frame(Some(missing)) != frame(Some(empty)));
        assert!(!frame(Some(missing)).get(54, 34));
        assert!(frame(Some(empty)).get(54, 34));
        assert!(parse(
            EXAMPLE,
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::from_secs(4)
        )
        .is_none());
        for line in [
            "ship 2 hull 74 shield 38 energy 78 ttl 3",
            "ship 1 hull 74 shield 0 ttl 3",
            "ship 1 hull 1000 shield none energy 78 ttl 3",
            "ship 1 hull 74 shield 38 energy NaN ttl 3",
            "ship 1 hull 74 shield 38 energy 78 ttl 0",
        ] {
            assert!(parsed(line).is_none(), "{line}");
        }
        assert!(parsed(&format!("{EXAMPLE} hull 99")).is_none());
    }
    #[test]
    fn lightning_pixels_invert_full_and_empty_pies() {
        let full = frame(Some(State {
            hull: 74,
            shield: Some(38),
            energy: Some(100),
            ..Default::default()
        }));
        let empty = frame(Some(State {
            hull: 74,
            shield: Some(38),
            energy: Some(0),
            ..Default::default()
        }));
        // These points are inside the disc: (27,16) is bolt; (26,28) is clear.
        assert!(!full.get(27, 16));
        assert!(empty.get(27, 16));
        assert!(full.get(26, 28));
        assert!(!empty.get(26, 28));
        // A static ship page has no dependence on a beat, tick or wall clock.
        assert!(
            full == frame(Some(State {
                hull: 74,
                shield: Some(38),
                energy: Some(100),
                ..Default::default()
            }))
        );
    }
    #[test]
    fn structured_records_round_trip_and_reject_single_field_mutations() {
        let mut entropy = [0; 8];
        fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut entropy)
            .unwrap();
        let mut seed = u64::from_ne_bytes(entropy) | 1;
        eprintln!("ship parser seed {seed}");
        for _ in 0..2000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let hull = seed % 1000;
            let energy = (seed >> 10) % 101;
            let line = format!("ship 1 energy {energy} shield none hull {hull} ttl 3");
            assert_eq!(
                parsed(&line),
                Some(State {
                    hull: hull as u32,
                    energy: Some(energy as u32),
                    shield: None,
                    ..Default::default()
                })
            );
            assert!(parsed(&format!("{line} energy 0")).is_none());
            assert!(parsed(&line.replace(&format!("hull {hull}"), "hull -1")).is_none());
        }
    }

    #[test]
    fn critical_hull_uses_steady_band_colour_without_pulses() {
        let _sandbox = crate::test_support::Sandbox::new("ship-critical");
        let path = crate::state_dir().join("ship");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "ship 1 hull 20 shield none energy 80 ttl 3").unwrap();
        let tuning = crate::meter::Tuning::default();
        let expected = paint(&tuning, [0, 0, 255]);
        assert_eq!(expected.1, [255, 0, 0]);
        for _ in 0..200 {
            let next = paint(&tuning, [0, 0, 255]);
            assert_eq!(next.1, expected.1);
            assert!(next.0 == expected.0);
        }
    }

    #[test]
    fn planning_fields_are_bounded_and_keep_absence_distinct() {
        let line = format!("{EXAMPLE} repair 12 shield_delay 8 energy_time -6");
        let state = parsed(&line).unwrap();
        assert_eq!(
            (state.repair, state.shield_delay, state.energy_time),
            (Some(12), Some(8), Some(-6))
        );
        for bad in [
            line.replace("repair 12", "repair 101"),
            line.replace("shield_delay 8", "shield_delay 10000"),
            line.replace("energy_time -6", "energy_time -10000"),
            line.replace("shield 38", "shield none"),
            line.replace("energy 78", "energy none"),
            format!("{line} repair 3"),
        ] {
            assert!(parsed(&bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn configured_feed_path_reads_and_clears_without_falling_back() {
        let _sandbox = crate::test_support::Sandbox::new("ship-path");
        let custom = crate::config_dir().join("a custom ship feed");
        fs::create_dir_all(crate::config_dir()).unwrap();
        fs::write(
            crate::config_dir().join("ship-feed"),
            format!("{}\n", custom.display()),
        )
        .unwrap();
        fs::write(&custom, EXAMPLE).unwrap();
        assert_eq!(read(), parsed(EXAMPLE));
        fs::write(&custom, "wait ttl 3").unwrap();
        assert!(read().is_none());
        fs::write(&custom, "x".repeat(513)).unwrap();
        assert!(read().is_none());
    }
    #[test]
    fn live_critical_display_stays_quiet_after_first_red_frame() {
        let _sandbox = crate::test_support::Sandbox::new("ship-steady-live");
        let pipe = crate::test_support::PanelPipe::new();
        fs::create_dir_all(crate::state_dir()).unwrap();
        fs::write(
            crate::state_dir().join("ship"),
            "ship 1 hull 20 shield none energy 80 ttl 30",
        )
        .unwrap();
        let live = crate::panel_feed::Live::start([0, 0, 255], "test", paint);
        let mut output = pipe.read();
        if output.len() < crate::lcd::BYTES {
            output.extend(pipe.read());
        }
        assert!(output.windows(12).any(|s| s == b"rgb 255 0 0\n"));
        for _ in 0..14 {
            pipe.expect_quiet();
        } // Includes a full two-second blink cycle.
        drop(live);
        let _ = pipe.read(); // restore resting blue
        pipe.expect_quiet();
    }
}
