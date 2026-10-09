// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded DFHack fortress feed and fixed LCD overview.
use super::{text, Label};
use crate::draw::Draw;
use crate::lcd::{Bitmap, H, W};
use std::time::{Duration, Instant, SystemTime};
use std::{fs, path::Path};

const SCRIPT: &str = include_str!("../contrib/df-health/g13-lcd.lua");
const INIT: &str = "# Managed by g13pad: fortress LCD overview\ng13-lcd start\n";

fn managed_files(root: &Path) -> Result<[(std::path::PathBuf, &'static str); 2], String> {
    if !root.join("hack/scripts").is_dir() || !root.join("dfhack-config").is_dir() {
        return Err(format!(
            "{} is not a Dwarf Fortress directory with DFHack installed",
            root.display()
        ));
    }
    Ok([
        (root.join("hack/scripts/g13-lcd.lua"), SCRIPT),
        (root.join("dfhack-config/init/dfhack.g13-lcd.init"), INIT),
    ])
}

fn check_owned(path: &Path, contents: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(meta) if !meta.file_type().is_file() => Err(format!(
            "{} is not a regular managed file; preserving it",
            path.display()
        )),
        Ok(_) => {
            let found = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            if found == contents {
                Ok(true)
            } else {
                Err(format!("{} differs from this release; preserving it. Back up or remove it before replacing it", path.display()))
            }
        }
    }
}

/// Install a separate init file without editing DFHack's existing startup configuration.
pub fn dfhack_install(root: &Path) -> Result<String, String> {
    let files = managed_files(root)?;
    // Validate both before changing either; edited scripts must never be overwritten.
    let present = files
        .iter()
        .map(|(p, s)| check_owned(p, s))
        .collect::<Result<Vec<_>, _>>()?;
    for ((path, contents), exists) in files.iter().zip(present) {
        if exists {
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(format!("Installed DFHack fortress overview in {}. Restart DFHack or run g13-lcd start in its console. Set the game's G13 profile to health mode feed. Remove with: g13map health dfhack DIR remove", root.display()))
}

/// Remove only the exact files installed by this release, preserving user edits.
pub fn dfhack_remove(root: &Path) -> Result<String, String> {
    let files = managed_files(root)?;
    let present = files
        .iter()
        .map(|(p, s)| check_owned(p, s))
        .collect::<Result<Vec<_>, _>>()?;
    for ((path, _), exists) in files.iter().zip(present) {
        if exists {
            fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok("Removed the managed DFHack LCD files. In an already running game, run g13-lcd stop before removal or restart DFHack.".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fortress {
    pub session: u64,
    pub population: u32,
    pub care: u32,
    pub infected: u32,
    pub walking: u32,
    pub hands: u32,
    /// Unique people with impaired standing or grasping limbs; absent in v1.
    pub limbs: Option<u32>,
    pub rags: u32,
    pub worn: u32,
    pub weather: Weather,
    /// Current sampled surface tile temperature in DF units, not worldgen units.
    pub temperature: Option<u16>,
    pub freeze: Freeze,
    pub severity: u8,
    /// Monotonic event sequence within this session. Persistent problems do not flash.
    pub alert: u64,
    pub detail: Label,
    /// Collector's detail phase, so one pass holds its tail until the next phase.
    pub detail_phase: Option<u64>,
    /// Calendar season; absent in older feeds.
    pub season: Option<Season>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}
impl Season {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "spring" => Some(Self::Spring),
            "summer" => Some(Self::Summer),
            "autumn" => Some(Self::Autumn),
            "winter" => Some(Self::Winter),
            _ => None,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Autumn => "autumn",
            Self::Winter => "winter",
        }
    }
    pub(super) fn rgb(self) -> [u8; 3] {
        match self {
            Self::Spring => [255, 0, 120],
            Self::Summer => [33, 234, 0],
            Self::Autumn => [249, 28, 0],
            Self::Winter => [0, 188, 163],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Unknown,
    Clear,
    Rain,
    Snow,
    Mixed,
    Off,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freeze {
    Unknown,
    None,
    Cold,
    Freezing,
    Frozen,
    Off,
}

impl Weather {
    fn word(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Snow => "snow",
            Self::Mixed => "mixed",
            Self::Off => "off",
        }
    }
}
impl Freeze {
    fn word(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::Cold => "cold",
            Self::Freezing => "freezing",
            Self::Frozen => "frozen",
            Self::Off => "off",
        }
    }
}
impl Fortress {
    pub(super) fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<Self> {
        if line.len() > 1024 {
            return None;
        }
        let mut words = line.split_whitespace();
        if words.next()? != "fort" {
            return None;
        }
        let version = words.next()?;
        if !matches!(version, "1" | "2") {
            return None;
        }
        const KEYS: [&str; 18] = [
            "session", "pop", "care", "inf", "walk", "hand", "rags", "worn", "weather", "temp",
            "freeze", "severity", "alert", "detail", "ttl", "limbs", "phase", "season",
        ];
        let mut values = [None; 18];
        while let Some(key) = words.next() {
            let index = KEYS.iter().position(|k| *k == key)?;
            if values[index].is_some() {
                return None;
            }
            values[index] = Some(words.next()?);
        }
        let get = |i: usize| values[i];
        let count = |i| get(i)?.parse::<u32>().ok().filter(|n| *n <= 1_000_000);
        let ttl = get(14)?
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=60).contains(n))?;
        if now.duration_since(written).unwrap_or_default() > Duration::from_secs(ttl.into()) {
            return None;
        }
        let result = Self {
            session: get(0)?.parse().ok()?,
            population: count(1)?,
            care: count(2)?,
            infected: count(3)?,
            walking: count(4)?,
            hands: count(5)?,
            limbs: match version {
                "2" => Some(count(15)?),
                _ if get(15).is_none() => None,
                _ => return None,
            },
            rags: count(6)?,
            worn: count(7)?,
            weather: match get(8)? {
                "unknown" => Weather::Unknown,
                "clear" => Weather::Clear,
                "rain" => Weather::Rain,
                "snow" => Weather::Snow,
                "mixed" => Weather::Mixed,
                "off" => Weather::Off,
                _ => return None,
            },
            temperature: match get(9)? {
                "unknown" => None,
                value => Some(
                    value
                        .parse::<u16>()
                        .ok()
                        .filter(|n| (1..=60_000).contains(n))?,
                ),
            },
            freeze: match get(10)? {
                "unknown" => Freeze::Unknown,
                "none" => Freeze::None,
                "cold" => Freeze::Cold,
                "freezing" => Freeze::Freezing,
                "frozen" => Freeze::Frozen,
                "off" => Freeze::Off,
                _ => return None,
            },
            severity: get(11)?.parse::<u8>().ok().filter(|n| *n <= 2)?,
            alert: get(12)?.parse().ok()?,
            detail: Label::parse(get(13)?)?,
            detail_phase: match version {
                "2" => Some(get(16)?.parse().ok()?),
                _ if get(16).is_none() => None,
                _ => return None,
            },
            season: match get(17) {
                Some(word) if version == "2" => Some(Season::parse(word)?),
                None => None,
                _ => return None,
            },
        };
        if [
            result.care,
            result.infected,
            result.walking,
            result.hands,
            result.worn,
        ]
        .iter()
        .any(|n| *n > result.population)
            || result.rags > result.worn
            || result.infected > result.care
            || result.limbs.is_some_and(|n| {
                n > result.population
                    || n < result.walking.max(result.hands)
                    || n > result.walking + result.hands
            })
        {
            return None;
        }
        Some(result)
    }

    pub(super) fn to_line(self) -> String {
        let mut line = format!("fort {} session {} pop {} care {} inf {} walk {} hand {} rags {} worn {} weather {} temp {} freeze {} severity {} alert {} detail {}",
            if self.limbs.is_some() { 2 } else { 1 },
            self.session, self.population, self.care, self.infected, self.walking, self.hands,
            self.rags, self.worn, self.weather.word(),
            self.temperature.map_or_else(|| "unknown".into(), |t| t.to_string()),
            self.freeze.word(), self.severity, self.alert, self.detail.as_str());
        if let Some(limbs) = self.limbs {
            line.push_str(&format!(
                " limbs {limbs} phase {}",
                self.detail_phase.unwrap_or(0)
            ));
            if let Some(season) = self.season {
                line.push_str(&format!(" season {}", season.word()));
            }
        }
        line
    }
}

#[derive(Default)]
pub(super) struct Display {
    session: Option<u64>,
    alert: u64,
    flash: Option<Instant>,
    pub(super) last_flash: Option<Instant>,
    detail: Label,
    detail_phase: Option<u64>,
    started: Option<Instant>,
    flash_active: bool,
}

fn count(n: u32) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}K", n / 1000)
    } else {
        "1M".into()
    }
}

// 9x9 monochrome adaptation of Bachsau's CC0 dwarf icon:
// https://commons.wikimedia.org/wiki/File:Dwarf_Fortress_Icon.svg
const DWARF: &[&str] = &[
    "..#####..",
    ".#.....#.",
    ".#.#.#.#.",
    ".#.....#.",
    ".#######.",
    ".##.#.##.",
    "..#####..",
    "....##...",
    ".....#...",
];

impl Display {
    pub(super) fn is_flashing(&self) -> bool {
        self.flash_active
    }
    pub(super) fn draw(&mut self, bm: &mut Bitmap, f: Fortress, now: Instant) {
        if self.session != Some(f.session) {
            self.session = Some(f.session);
            self.alert = f.alert;
            self.flash = None; // First snapshot is a baseline, not a newly observed event.
            self.detail = Label::EMPTY;
        } else if f.alert > self.alert {
            self.alert = f.alert;
            if f.severity > 0
                && self
                    .last_flash
                    .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(6))
            {
                self.flash = Some(now);
                self.last_flash = Some(now);
            }
        }
        let weather = match f.weather {
            Weather::Unknown => "WX --".into(),
            Weather::Off => "WX OFF".into(),
            w => w.word().to_uppercase(),
        };
        let temperature = f.temperature.map_or_else(
            || "--°C".into(),
            |t| {
                format!(
                    "{}°C",
                    ((i32::from(t) - 10_000) as f64 * 5.0 / 9.0).round() as i32
                )
            },
        );
        bm.sprite(0, 0, DWARF);
        text(bm, 12, 1, &count(f.population), 1);
        text(bm, 60, 1, &weather, 1);
        let width = temperature.chars().count() as i32 * 6 - 1;
        text(bm, W as i32 - width, 1, &temperature, 1);
        for (y, row) in [
            (
                10,
                format!("CARE {} ({} INF)", count(f.care), count(f.infected)),
            ),
            (
                18,
                f.limbs.map_or_else(
                    || format!("LIMBS W{} H{}", count(f.walking), count(f.hands)),
                    |n| {
                        format!(
                            "LIMBS {} (W{} H{})",
                            count(n),
                            count(f.walking),
                            count(f.hands)
                        )
                    },
                ),
            ),
            (
                26,
                format!("CLOTHES {} ({} RAGS)", count(f.worn), count(f.rags)),
            ),
        ] {
            text(bm, 0, y, &row, 1);
        }
        bm.line(0, 34, W as i32 - 1, 34);
        if f.detail != self.detail || f.detail_phase != self.detail_phase {
            self.detail = f.detail;
            self.detail_phase = f.detail_phase;
            self.started = Some(now);
        }
        let detail = f.detail.as_str().replace('_', " ").to_uppercase();
        let length = detail.len() as u64 * 6;
        if length <= W as u64 {
            text(bm, 0, 36, &detail, 1);
        } else {
            // 1s at the beginning, 20 pixels/s, then hold the last visible
            // section. The collector advances the phase after an end pause.
            let end_offset = length - W as u64;
            let elapsed = now
                .saturating_duration_since(self.started.unwrap_or(now))
                .as_millis();
            let phase_ms = if f.detail_phase.is_some() {
                elapsed.min(u128::from(1000 + end_offset * 50))
            } else {
                // Older feeders have no phase counter: repeat with a 1s end pause.
                elapsed % u128::from(2000 + end_offset * 50)
            };
            let offset = (phase_ms.saturating_sub(1000) / 50).min(u128::from(end_offset)) as i32;
            text(bm, -offset, 36, &detail, 1);
        }
        self.flash_active = self
            .flash
            .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(300));
        if self.flash_active {
            for y in 0..H {
                for x in 0..W {
                    bm.set(x, y, true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line() -> &'static str {
        "fort 1 session 1 pop 84 care 6 inf 2 walk 3 hand 1 rags 17 worn 26 weather snow temp 9998 freeze freezing severity 2 alert 0 detail URIST_NEEDS_CRUTCH ttl 6"
    }
    fn sample() -> Fortress {
        Fortress::parse(line(), SystemTime::UNIX_EPOCH, SystemTime::UNIX_EPOCH).unwrap()
    }
    #[test]
    fn grouped_layout_preserves_degree_and_count_glyphs() {
        let mut f = sample();
        f.population = 116;
        f.temperature = Some(10015);
        f.limbs = Some(3);
        let mut d = Display::default();
        let mut b = Bitmap::blank();
        d.draw(&mut b, f, Instant::now());
        // The Unicode degree occupies one glyph cell; it must not push C offscreen.
        let mut expected = Bitmap::blank();
        text(&mut expected, 143, 1, "8°C", 1);
        for y in 1..8 {
            for x in 143..W {
                assert_eq!(b.get(x, y), expected.get(x, y));
            }
        }
        assert!(b.get(3, 2) && b.get(5, 2)); // Dwarf's eyes survive the downscale.
        let mut expected = Bitmap::blank();
        text(&mut expected, 0, 18, "LIMBS 3 (W3 H1)", 1);
        for y in 18..25 {
            for x in 0..W {
                assert_eq!(b.get(x, y), expected.get(x, y));
            }
        }
    }
    #[test]
    fn installer_preserves_existing_configuration_and_edited_files() {
        let root = std::env::temp_dir().join(format!("g13-dfhack-install-{}", std::process::id()));
        fs::create_dir_all(root.join("hack/scripts")).unwrap();
        fs::create_dir_all(root.join("dfhack-config/init")).unwrap();
        let user = root.join("dfhack-config/init/dfhack.init");
        fs::write(&user, "my existing startup commands\n").unwrap();
        dfhack_install(&root).unwrap();
        dfhack_install(&root).unwrap();
        let script = root.join("hack/scripts/g13-lcd.lua");
        let init = root.join("dfhack-config/init/dfhack.g13-lcd.init");
        fs::write(&script, "user edits").unwrap();
        assert!(dfhack_install(&root).is_err());
        assert!(dfhack_remove(&root).is_err());
        assert_eq!(fs::read_to_string(&script).unwrap(), "user edits");
        assert!(init.exists()); // Both are checked before removing either.
        fs::write(&script, SCRIPT).unwrap();
        dfhack_remove(&root).unwrap();
        assert!(!script.exists() && !init.exists());
        assert_eq!(
            fs::read_to_string(user).unwrap(),
            "my existing startup commands\n"
        );
        let outside = root.join("outside.lua");
        fs::write(&outside, SCRIPT).unwrap();
        std::os::unix::fs::symlink(&outside, &script).unwrap();
        assert!(dfhack_install(&root).is_err());
        assert!(outside.exists());
        assert!(!init.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_or_incomplete_snapshots_never_become_healthy_counts() {
        let t = SystemTime::UNIX_EPOCH;
        assert!(Fortress::parse(line(), t, t).is_some());
        for bad in [
            line().replace("fort 1", "fort 3"),
            line().replace("pop 84", "pop 1"),
            line().replace("rags 17", "rags 27"),
            line().replace("temp 9998", "temp 60001"),
            line().replace("temp 9998", "temp 0"),
            line().replace("care 6", "care 1"),
            line().replace("inf 2", "inf -1"),
            line().replace(" care 6", ""),
            line().replace("weather snow", "weather surprise"),
            format!("{} pop 84", line()),
            line().replace("ttl 6", "ttl 0"),
        ] {
            assert!(Fortress::parse(&bad, t, t).is_none(), "{bad}");
        }
        assert!(Fortress::parse(line(), t, t + Duration::from_secs(7)).is_none());
        let f = sample();
        assert_eq!(Fortress::parse(&(f.to_line() + " ttl 6"), t, t), Some(f));
        let v2 = line().replace("fort 1", "fort 2") + " limbs 3 phase 0";
        let f = Fortress::parse(&v2, t, t).unwrap();
        assert_eq!(f.limbs, Some(3));
        assert_eq!(Fortress::parse(&(f.to_line() + " ttl 6"), t, t), Some(f));
        for bad in [
            line().to_owned() + " limbs 3",
            line().replace("fort 1", "fort 2"),
            v2.replace("limbs 3", "limbs 2"),
            v2.replace("limbs 3", "limbs 5"),
            v2.replace(" phase 0", ""),
            format!("{v2} season unknown"),
            format!("{v2} season winter season summer"),
            format!("{} season spring", line()),
        ] {
            assert!(Fortress::parse(&bad, t, t).is_none(), "{bad}");
        }
    }
    #[test]
    fn season_colours_return_after_alerts_and_changes_do_not_trigger_them() {
        use super::super::{Meter, State};
        let t = SystemTime::UNIX_EPOCH;
        let v2 = line().replace("fort 1", "fort 2") + " limbs 3 phase 0";
        let start = Instant::now();
        let mut meter = Meter::default();
        for (word, rgb) in [
            ("spring", [255, 0, 120]),
            ("summer", [33, 234, 0]),
            ("autumn", [249, 28, 0]),
            ("winter", [0, 188, 163]),
        ] {
            let f = Fortress::parse(&format!("{v2} season {word}"), t, t).unwrap();
            assert_eq!(Fortress::parse(&(f.to_line() + " ttl 6"), t, t), Some(f));
            assert!(!filled(&meter.frame_at(State::Fortress(f), start)));
            assert_eq!(meter.colour(State::Fortress(f), [1, 2, 3]), rgb);
        }
        let mut f = Fortress::parse(&format!("{v2} season winter"), t, t).unwrap();
        f.alert = 1;
        assert!(filled(
            &meter.frame_at(State::Fortress(f), start + Duration::from_secs(2))
        ));
        assert_eq!(
            meter.colour(State::Fortress(f), [1, 2, 3]),
            meter.tuning.alarm().rgb
        );
        meter.frame_at(State::Fortress(f), start + Duration::from_millis(2299));
        assert_eq!(
            meter.colour(State::Fortress(f), [1, 2, 3]),
            meter.tuning.alarm().rgb
        );
        meter.frame_at(State::Fortress(f), start + Duration::from_millis(2300));
        assert_eq!(meter.colour(State::Fortress(f), [1, 2, 3]), [0, 188, 163]);
        f.alert = 2;
        f.season = Some(Season::Spring);
        assert!(!filled(
            &meter.frame_at(State::Fortress(f), start + Duration::from_secs(3))
        ));
        assert_eq!(meter.colour(State::Fortress(f), [1, 2, 3]), [255, 0, 120]);
        f.alert = 3;
        assert!(filled(
            &meter.frame_at(State::Fortress(f), start + Duration::from_secs(8))
        ));
        assert_eq!(
            meter.colour(State::Fortress(f), [1, 2, 3]),
            meter.tuning.alarm().rgb
        );
    }
    #[test]
    fn fortress_replaces_death_hold_and_uses_urgency_colours_without_a_heartbeat() {
        use super::super::{Meter, State};
        let start = Instant::now();
        let mut meter = Meter::default();
        meter.frame_at(State::health(0), start);
        assert!(meter.holding());
        let mut f = sample();
        let first = meter.frame_at(State::Fortress(f), start);
        assert!(!meter.holding());
        assert_eq!(
            first,
            meter.frame_at(State::Fortress(f), start + Duration::from_secs(1))
        );
        let rest = [19, 0, 127];
        assert_eq!(
            meter.colour(State::Fortress(f), rest),
            meter.tuning.alarm().rgb
        );
        f.severity = 1;
        assert_eq!(
            meter.colour(State::Fortress(f), rest),
            meter.tuning.rgb_at(60)
        );
        f.severity = 0;
        assert_eq!(meter.colour(State::Fortress(f), rest), rest);
    }
    fn filled(b: &Bitmap) -> bool {
        (0..H).all(|y| (0..W).all(|x| b.get(x, y)))
    }
    #[test]
    fn persistent_conditions_and_save_switches_do_not_reflash() {
        let mut d = Display::default();
        let start = Instant::now();
        let mut f = sample();
        let mut render = |f, t| {
            let mut b = Bitmap::blank();
            d.draw(&mut b, f, t);
            b
        };
        assert!(!filled(&render(f, start)));
        f.alert = 1;
        assert!(filled(&render(f, start + Duration::from_secs(1))));
        assert!(!filled(&render(f, start + Duration::from_millis(1300))));
        assert!(!filled(&render(f, start + Duration::from_secs(8))));
        f.alert = 2;
        assert!(filled(&render(f, start + Duration::from_secs(8))));
        f.alert = 3;
        assert!(!filled(&render(f, start + Duration::from_secs(10))));
        f.alert = 2; // An older/out-of-order snapshot cannot replay an alert.
        assert!(!filled(&render(f, start + Duration::from_secs(16))));
        f.alert = 3;
        assert!(!filled(&render(f, start + Duration::from_secs(17))));
        f.session = 2;
        assert!(!filled(&render(f, start + Duration::from_secs(18))));
    }
    #[test]
    fn long_detail_scrolls_without_touching_overview() {
        let mut d = Display::default();
        let start = Instant::now();
        let mut f = sample();
        f.detail = Label::parse(&"URIST_NEEDS_A_CRUTCH_".repeat(10)).unwrap();
        let mut a = Bitmap::blank();
        d.draw(&mut a, f, start);
        let mut b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_millis(999));
        assert_eq!(a, b);
        b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_millis(1050));
        assert_ne!(a, b); // Scroll starts after the one-second leading pause.
        b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_secs(4));
        for y in 0..36 {
            for x in 0..W {
                assert_eq!(a.get(x, y), b.get(x, y));
            }
        }
        assert_ne!(a, b);
        b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_secs(86400 * 365 * 100));
        f.detail_phase = Some(1);
        b = Bitmap::blank();
        d.draw(&mut b, f, start);
        let mut tail = Bitmap::blank();
        d.draw(&mut tail, f, start + Duration::from_secs(100));
        b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_secs(101));
        assert_eq!(b, tail); // Final section stays still until the next phase.
        f.detail_phase = Some(2);
        b = Bitmap::blank();
        d.draw(&mut b, f, start + Duration::from_secs(102));
        assert_eq!(b, a); // Same text can restart without changing the overview.
    }
}
