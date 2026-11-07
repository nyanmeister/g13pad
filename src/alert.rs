// SPDX-License-Identifier: GPL-3.0-or-later
//! Off-screen alerts: a compass to something that just happened out of view, with a short
//! word for what it was. A game's feeder (a widget plus a relay) writes one line; this
//! screen knows bearings, labels and severity, never the game. Chosen 2026-10-27 as the
//! compass layout over a word stack and a minimap (G13 Review/mockup-20261027).
//!
//! Feed, one line in `$XDG_RUNTIME_DIR/g13map-g13-0.alert` or `~/.local/state/g13map/alert`:
//!
//! ```text
//! alert BEARING LABEL [kind hit|lost] [sev 1-5] [at EPOCH_MS] [ttl SECONDS]
//! quiet [ttl SECONDS]
//! off
//! ```
//!
//! `BEARING` is degrees clockwise from north (the top of the map). `LABEL` is one token,
//! underscores for spaces, shown upper-case and cut to eight glyphs. `at` is when it
//! happened, so a keepalive rewrite does not reset the "ago" clock. `quiet` means the feeder
//! is alive with nothing to show; no file, an expired `ttl` or `off` means no feeder.
use crate::{draw::Draw, lcd::Bitmap, meter::text};
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

/// Longest label the panel shows at the big size beside the dial.
pub const MAX_LABEL: usize = 8;
/// How long the backlight carries the alert colour after the event.
const FLASH: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hit,
    Lost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub bearing: u16,
    pub label: String,
    pub kind: Kind,
    pub sev: u8,
    pub at: SystemTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Quiet,
    Alert(Alert),
}

fn clean_label(word: &str) -> String {
    let cleaned: String = word
        .replace('_', " ")
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == ' ')
        .collect();
    let trimmed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed
        .chars()
        .take(MAX_LABEL)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// One feed line, given when it was written. `None` for `off`, expiry, or a line that
/// does not parse: a broken feeder is no feeder.
pub fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    let mut words = line.split_whitespace();
    let head = words.next()?;
    let mut alert = match head {
        "off" => return None,
        "quiet" => None,
        "alert" => {
            let bearing = words.next()?.parse::<u32>().ok()? % 360;
            let label = clean_label(words.next()?);
            if label.is_empty() {
                return None;
            }
            Some(Alert {
                bearing: bearing as u16,
                label,
                kind: Kind::Hit,
                sev: 3,
                at: written,
            })
        }
        _ => return None,
    };
    let mut seen_ttl = false;
    while let Some(key) = words.next() {
        let value = words.next()?;
        match (key, alert.as_mut()) {
            ("kind", Some(a)) => {
                a.kind = match value {
                    "hit" => Kind::Hit,
                    "lost" => Kind::Lost,
                    _ => return None,
                }
            }
            ("sev", Some(a)) => a.sev = value.parse::<u8>().ok()?.clamp(1, 5),
            ("at", Some(a)) => {
                let ms = value.parse::<u64>().ok()?;
                a.at = SystemTime::UNIX_EPOCH + Duration::from_millis(ms);
            }
            ("ttl", _) => {
                if seen_ttl {
                    return None;
                }
                seen_ttl = true;
                crate::feed::fresh(value, written, now)?;
            }
            _ => return None,
        }
    }
    Some(match alert {
        Some(a) => State::Alert(a),
        None => State::Quiet,
    })
}

/// Where the feed lives: the runtime file and the state-dir file; the newest wins.
pub fn paths() -> Vec<std::path::PathBuf> {
    crate::feed::pair("alert")
}

pub fn read() -> Option<State> {
    crate::feed::newest(paths(), &["off"], parse)
}

/// Writes one line to the runtime feed file, atomically.
pub fn write(line: &str) -> Result<(), String> {
    crate::feed::write_line(&crate::daemon::runtime_file("alert")?, line)
}

/// The eight compass names, clockwise from north.
pub const SECTORS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

pub fn sector(bearing: u16) -> &'static str {
    SECTORS[(((bearing as u32 + 22) % 360) / 45) as usize]
}

// The dial: left of the words, the whole height of the glass.
const CX: i32 = 20;
const CY: i32 = 21;
const R: i32 = 17;
/// Where the words start.
const TX: i32 = 44;

fn dial(bm: &mut Bitmap) {
    for a in 0..360 {
        let t = (a as f32).to_radians();
        bm.plot(
            CX + (R as f32 * t.sin()).round() as i32,
            CY - (R as f32 * t.cos()).round() as i32,
        );
    }
    // North is marked with a two-pixel notch; the others with one.
    bm.plot(CX, CY - R + 1);
    bm.plot(CX, CY - R + 2);
    bm.plot(CX + R - 1, CY);
    bm.plot(CX, CY + R - 1);
    bm.plot(CX - R + 1, CY);
}

/// A thick arrow from the centre toward `bearing`, ending short of the rim.
fn arrow(bm: &mut Bitmap, bearing: u16) {
    let t = (bearing as f32).to_radians();
    let (dx, dy) = (t.sin(), -t.cos());
    let len = (R - 4) as f32;
    let tip = (CX as f32 + dx * len, CY as f32 + dy * len);
    // Perpendicular offsets make the shaft three pixels wide.
    let (px, py) = (-dy, dx);
    for o in [-1.0f32, 0.0, 1.0] {
        bm.line(
            (CX as f32 + px * o).round() as i32,
            (CY as f32 + py * o).round() as i32,
            (tip.0 + px * o).round() as i32,
            (tip.1 + py * o).round() as i32,
        );
    }
    // Head: two barbs swept back from the tip.
    for side in [-1.0f32, 1.0] {
        let ang = t + side * 2.6;
        let (bx, by) = (ang.sin(), -ang.cos());
        for o in [0.0f32, 1.0] {
            bm.line(
                (tip.0 + px * o * side).round() as i32,
                (tip.1 + py * o * side).round() as i32,
                (tip.0 + bx * 7.0 + px * o * side).round() as i32,
                (tip.1 + by * 7.0 + py * o * side).round() as i32,
            );
        }
    }
}

fn ago(at: SystemTime, now: SystemTime) -> String {
    let s = now.duration_since(at).unwrap_or_default().as_secs();
    if s < 60 {
        format!("{s}s AGO")
    } else if s < 3600 {
        format!("{}m AGO", s / 60)
    } else {
        "LONG AGO".into()
    }
}

pub fn frame(state: Option<&State>, now: SystemTime) -> Bitmap {
    let mut bm = Bitmap::blank();
    dial(&mut bm);
    match state {
        None => {
            text(&mut bm, TX, 8, "NO FEED", 2);
            text(&mut bm, TX, 30, "WIDGET OFF", 1);
        }
        Some(State::Quiet) => {
            text(&mut bm, TX, 8, "QUIET", 2);
            text(&mut bm, TX, 30, "ALL CLEAR", 1);
        }
        Some(State::Alert(a)) => {
            arrow(&mut bm, a.bearing);
            text(&mut bm, TX, 3, &a.label, 2);
            let verb = match a.kind {
                Kind::Hit => "HIT",
                Kind::Lost => "LOST",
            };
            text(
                &mut bm,
                TX,
                22,
                &format!("{verb}  {}", sector(a.bearing)),
                1,
            );
            text(&mut bm, TX, 32, &ago(a.at, now), 1);
            // Severity as bars at the right edge, one per level.
            for i in 0..a.sev as i32 {
                bm.fill(155, 35 - i * 8, 3, 6, true);
            }
        }
    }
    bm
}

/// The backlight: the profile's own colour, overridden for a few seconds after an alert
/// by its severity. Saturated hues, since the lamp is one additive LED.
pub fn colour(state: Option<&State>, now: SystemTime, rest: [u8; 3]) -> [u8; 3] {
    match state {
        Some(State::Alert(a)) if now.duration_since(a.at).unwrap_or_default() < FLASH => {
            match a.sev {
                5 | 4 => [255, 0, 0],
                3 | 2 => [255, 60, 0],
                _ => [255, 200, 0],
            }
        }
        _ => rest,
    }
}

pub(crate) fn paint(_tuning: &crate::meter::Tuning, rest: [u8; 3]) -> (Bitmap, [u8; 3]) {
    let state = read();
    let now = SystemTime::now();
    (
        frame(state.as_ref(), now),
        colour(state.as_ref(), now, rest),
    )
}

const USAGE: &str = "usage: g13map alert feed LINE | relay FILE | demo";

pub(crate) fn command(args: &[&str]) -> Result<String, String> {
    match args {
        ["feed", line] => write(line).map(|_| String::new()),
        ["relay", file] => relay(Path::new(file)),
        ["demo"] => demo(),
        _ => Err(USAGE.into()),
    }
}

/// Copies a game-side file into the feed whenever it changes. A widget sandboxed to its
/// game's own folder writes the feed grammar there; this moves it where the watcher looks.
/// Runs until killed; a missing file is simply nothing yet.
fn relay(file: &Path) -> Result<String, String> {
    let mut last: Option<(SystemTime, u64)> = None;
    let mut last_error = String::new();
    loop {
        let stamp = fs::metadata(file)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        if stamp.is_some() && stamp != last {
            last = stamp;
            let result = fs::read_to_string(file)
                .map_err(|e| format!("{}: {e}", file.display()))
                .and_then(|text| {
                    let line = text.lines().next().unwrap_or("").trim().to_string();
                    if line.is_empty() {
                        return Ok(());
                    }
                    // Validate before forwarding; a line the parser rejects is logged, not fed.
                    let now = SystemTime::now();
                    if parse(&line, now, now).is_none() && line != "off" {
                        return Err(format!("not a feed line: {line}"));
                    }
                    write(&line)
                });
            if let Err(e) = result {
                if e != last_error {
                    eprintln!("alert relay: {e}");
                    last_error = e;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Plays a short sequence through the real feed, to look at on the glass.
fn demo() -> Result<String, String> {
    let steps: [(&str, u64); 5] = [
        ("quiet ttl 30", 2),
        ("alert 225 COM kind hit sev 5 ttl 30", 5),
        ("alert 40 FACTORY kind lost sev 3 ttl 30", 5),
        ("alert 350 LOTUS kind hit sev 1 ttl 30", 5),
        ("quiet ttl 30", 2),
    ];
    for (line, hold) in steps {
        let line = if line.starts_with("alert") {
            format!("{line} at {}", now_ms())
        } else {
            line.to_string()
        };
        write(&line)?;
        std::thread::sleep(Duration::from_secs(hold));
    }
    write("off")?;
    Ok("alert demo done".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn parses_the_full_line_and_defaults() {
        let s = parse(
            "alert 225 COM_HQ kind lost sev 5 at 1000000 ttl 8",
            at(1000),
            at(1004),
        )
        .unwrap();
        let State::Alert(a) = s else { panic!() };
        assert_eq!(a.bearing, 225);
        assert_eq!(a.label, "COM HQ");
        assert_eq!(a.kind, Kind::Lost);
        assert_eq!(a.sev, 5);
        assert_eq!(a.at, at(1000));
        let State::Alert(b) = parse("alert 400 lotus", at(7), at(7)).unwrap() else {
            panic!()
        };
        assert_eq!(b.bearing, 40); // wrapped
        assert_eq!(b.kind, Kind::Hit);
        assert_eq!(b.sev, 3);
        assert_eq!(b.at, at(7)); // written time when no `at`
        assert_eq!(b.label, "LOTUS");
    }

    #[test]
    fn rejects_and_expires() {
        assert_eq!(parse("quiet", at(1), at(1)), Some(State::Quiet));
        assert_eq!(parse("quiet ttl 5", at(1), at(7)), None);
        assert_eq!(parse("alert 10 X ttl 5", at(1), at(6)).is_some(), true);
        assert_eq!(parse("alert 10 X ttl 5", at(1), at(7)), None);
        assert_eq!(parse("off", at(1), at(1)), None);
        assert_eq!(parse("alert 10", at(1), at(1)), None); // no label
        assert_eq!(parse("alert 10 !!!", at(1), at(1)), None); // label with no glyphs
        assert_eq!(parse("alert 10 X kind maybe", at(1), at(1)), None);
        assert_eq!(parse("alert 10 X bogus 1", at(1), at(1)), None);
        assert_eq!(parse("quiet sev 3", at(1), at(1)), None); // sev needs an alert
        assert_eq!(
            parse("alert 10 X sev 9", at(1), at(1)).map(|s| match s {
                State::Alert(a) => a.sev,
                _ => 0,
            }),
            Some(5)
        );
    }

    #[test]
    fn labels_are_cut_to_eight_glyphs() {
        assert_eq!(clean_label("strider_hub_of_doom"), "STRIDER");
        assert_eq!(clean_label("Solar"), "SOLAR");
        assert_eq!(clean_label("ab-cd"), "ABCD");
    }

    #[test]
    fn sectors_by_hand() {
        assert_eq!(sector(0), "N");
        assert_eq!(sector(22), "N");
        assert_eq!(sector(23), "NE");
        assert_eq!(sector(90), "E");
        assert_eq!(sector(225), "SW");
        // The north sector spans 337.5..22.5: 338 is north, 337 is not.
        assert_eq!(sector(338), "N");
        assert_eq!(sector(337), "NW");
    }

    #[test]
    fn arrow_points_where_the_bearing_says() {
        let a = Alert {
            bearing: 225,
            label: "COM".into(),
            kind: Kind::Hit,
            sev: 5,
            at: at(10),
        };
        let bm = frame(Some(&State::Alert(a)), at(12));
        // tip at 13 px toward SW from (20,21): (20-9.2, 21+9.2) ~ (11, 30)
        assert!(bm.get(11, 30), "tip pixel lit");
        // nothing of the arrow toward NE inside the dial
        assert!(!bm.get(27, 14), "NE quadrant stays dark");
        // the dial itself is present: north notch
        assert!(bm.get(20, 5));
        // words: the severity bars, five of them, at the right edge
        assert!(bm.get(156, 40) && bm.get(156, 8));
        let q = frame(Some(&State::Quiet), at(12));
        assert!(!q.get(11, 30), "no arrow when quiet");
        let n = frame(None, at(12));
        assert!(!n.get(156, 40), "no bars without a feed");
    }

    #[test]
    fn colour_flashes_then_rests() {
        let a = Alert {
            bearing: 0,
            label: "X".into(),
            kind: Kind::Hit,
            sev: 4,
            at: at(100),
        };
        let s = State::Alert(a);
        assert_eq!(colour(Some(&s), at(101), [1, 2, 3]), [255, 0, 0]);
        assert_eq!(colour(Some(&s), at(104), [1, 2, 3]), [1, 2, 3]);
        assert_eq!(colour(Some(&State::Quiet), at(104), [1, 2, 3]), [1, 2, 3]);
    }

    #[test]
    fn ago_wording() {
        assert_eq!(ago(at(10), at(12)), "2s AGO");
        assert_eq!(ago(at(10), at(130)), "2m AGO");
        assert_eq!(ago(at(10), at(5)), "0s AGO");
    }
}

#[cfg(test)]
mod dump {
    //! `G13MAP_ALERT_DUMP=DIR cargo test --lib alert::dump -- --ignored` writes every
    //! state as a PBM to look at (the mockup skill tints them); a no-op without the variable.
    use super::*;
    use crate::lcd::{H, W};

    #[test]
    #[ignore]
    fn frames() {
        let Some(dir) = std::env::var_os("G13MAP_ALERT_DUMP") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let mk = |b: u16, l: &str, k: Kind, sev: u8, ago: u64| {
            State::Alert(Alert {
                bearing: b,
                label: l.into(),
                kind: k,
                sev,
                at: now - Duration::from_secs(ago),
            })
        };
        let states: Vec<(&str, Option<State>)> = vec![
            ("nofeed", None),
            ("quiet", Some(State::Quiet)),
            ("com-sw", Some(mk(225, "COM", Kind::Hit, 5, 2))),
            ("fac-ne", Some(mk(40, "FACTORY", Kind::Lost, 3, 14))),
            ("lotus-n", Some(mk(350, "LOTUS", Kind::Hit, 1, 0))),
            ("solar-e", Some(mk(95, "SOLAR", Kind::Hit, 2, 7))),
        ];
        for (name, s) in states {
            let bm = frame(s.as_ref(), now);
            let mut pbm = format!("P1\n{W} {H}\n");
            for y in 0..H {
                for x in 0..W {
                    pbm.push(if bm.get(x, y) { '1' } else { '0' });
                    pbm.push(' ');
                }
                pbm.push('\n');
            }
            std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
        }
    }
}
