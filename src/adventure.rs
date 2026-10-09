// SPDX-License-Identifier: GPL-3.0-or-later
//! Adventure condition feed: categorical urgency, never synthetic character HP.
use super::{fortress, text, Label, Tuning};
use crate::draw::Draw;
use crate::lcd::{Bitmap, W};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adventure {
    pub session: u64,
    pub unit: u32,
    pub name: Label,
    pub condition: Label,
    pub effort: Label,
    pub blood: Option<u8>,
    pub wounds: u32,
    pub walk: bool,
    pub hands: bool,
    pub allies: [u32; 3], // affected, total, unavailable
    pub pets: [u32; 3],
    pub severity: u8, // ready, caution, impaired, immediate danger, dead
    pub alert: u64,
    pub phase: u64,
    pub detail: Label,
}
impl Adventure {
    pub(super) fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<Self> {
        if line.len() > 1536 {
            return None;
        }
        let mut words = line.split_whitespace();
        if words.next()? != "adv" || words.next()? != "1" {
            return None;
        }
        const KEYS: [&str; 16] = [
            "session",
            "unit",
            "name",
            "condition",
            "effort",
            "blood",
            "wounds",
            "walk",
            "hand",
            "allies",
            "pets",
            "severity",
            "alert",
            "phase",
            "detail",
            "ttl",
        ];
        let mut values = [None; 16];
        while let Some(key) = words.next() {
            let i = KEYS.iter().position(|k| *k == key)?;
            if values[i].is_some() {
                return None;
            }
            values[i] = Some(words.next()?);
        }
        let get = |i: usize| values[i];
        let ttl = get(15)?
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=60).contains(n))?;
        if now.duration_since(written).unwrap_or_default() > Duration::from_secs(ttl.into()) {
            return None;
        }
        let number = |i| get(i)?.parse::<u32>().ok().filter(|n| *n <= 1_000_000);
        let boolean = |i| match get(i)? {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        let party = |i| {
            let v = get(i)?
                .split('/')
                .map(|n| n.parse::<u32>().ok())
                .collect::<Option<Vec<_>>>()?;
            let v: [u32; 3] = v.try_into().ok()?;
            (v[1] <= 10000 && v[0] <= v[1] && v[2] <= v[1] - v[0]).then_some(v)
        };
        Some(Self {
            session: get(0)?.parse().ok()?,
            unit: get(1)?
                .parse::<u32>()
                .ok()
                .filter(|n| *n <= i32::MAX as u32)?,
            name: Label::parse(get(2)?)?,
            condition: Label::parse(get(3)?)?,
            effort: Label::parse(get(4)?)?,
            blood: match get(5)? {
                "na" => None,
                v => Some(v.parse::<u8>().ok().filter(|n| *n <= 100)?),
            },
            wounds: number(6)?,
            walk: boolean(7)?,
            hands: boolean(8)?,
            allies: party(9)?,
            pets: party(10)?,
            severity: get(11)?.parse::<u8>().ok().filter(|n| *n <= 4)?,
            alert: get(12)?.parse().ok()?,
            phase: get(13)?.parse().ok()?,
            detail: Label::parse(get(14)?)?,
        })
    }
    pub(super) fn to_line(self) -> String {
        format!("adv 1 session {} unit {} name {} condition {} effort {} blood {} wounds {} walk {} hand {} allies {}/{}/{} pets {}/{}/{} severity {} alert {} phase {} detail {}", self.session, self.unit, self.name.as_str(), self.condition.as_str(), self.effort.as_str(), self.blood.map_or_else(|| "na".into(), |v| v.to_string()), self.wounds, u8::from(self.walk), u8::from(self.hands), self.allies[0], self.allies[1], self.allies[2], self.pets[0], self.pets[1], self.pets[2], self.severity, self.alert, self.phase, self.detail.as_str())
    }
    pub(super) fn colour(self, tuning: &Tuning) -> [u8; 3] {
        let (name, fallback) = match self.severity {
            0 => ("green", [0, 255, 0]),
            1 => ("yellow", [255, 215, 0]),
            2 => ("orange", [255, 48, 0]),
            3 => ("red", [255, 0, 0]),
            _ => ("dead", [0, 0, 0]),
        };
        tuning
            .bands
            .iter()
            .find(|b| b.name == name)
            .map_or(fallback, |b| b.rgb)
    }
    pub(super) fn draw(self, display: &mut fortress::Display, bm: &mut Bitmap, now: Instant) {
        const RIGHT_COLUMN: i32 = 78;
        // Alerts also include a companion/pet emergency while the player is ready.
        display.begin(self.session, self.alert, 1, now);
        bm.sprite(0, 0, fortress::DWARF);
        let condition: String = self
            .condition
            .as_str()
            .replace('_', " ")
            .chars()
            .take(15)
            .collect();
        let right = W as i32 - condition.len() as i32 * 6 + 1;
        let name: String = self
            .name
            .as_str()
            .split('_')
            .next()
            .unwrap_or("")
            .chars()
            .take(((right - 15).max(0) / 6) as usize)
            .collect();
        text(bm, 12, 1, &name, 1);
        text(bm, right, 1, &condition, 1);
        let blood = self.blood.map_or_else(|| "--".into(), |v| format!("{v}%"));
        text(bm, 0, 10, &format!("BLOOD {blood}"), 1);
        let effort = self.effort.as_str().replace('_', " ");
        text(
            bm,
            RIGHT_COLUMN,
            10,
            &effort.chars().take(12).collect::<String>(),
            1,
        );
        text(
            bm,
            0,
            18,
            &format!("WALK {}", if self.walk { "OK" } else { "BAD" }),
            1,
        );
        text(
            bm,
            RIGHT_COLUMN,
            18,
            &format!("HANDS {}", if self.hands { "OK" } else { "BAD" }),
            1,
        );
        let party = |[affected, total, unknown]: [u32; 3]| {
            // Counts always fit their fixed half-row, including very large parties.
            let count = |v| {
                if v < 100 {
                    format!("{v}")
                } else {
                    "99+".into()
                }
            };
            if unknown > 0 {
                format!("{}?", count(total))
            } else {
                format!("{}/{}", count(affected), count(total))
            }
        };
        text(bm, 0, 26, &format!("ALLY {}", party(self.allies)), 1);
        text(
            bm,
            RIGHT_COLUMN,
            26,
            &format!("PETS {}", party(self.pets)),
            1,
        );
        bm.line(0, 34, W as i32 - 1, 34);
        display.detail(bm, self.detail, Some(self.phase), now);
        display.finish(bm, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::{parse, Meter, State};
    fn line() -> &'static str {
        "adv 1 session 10 unit 281 name GUKI condition READY effort RESTED blood 100 wounds 0 walk 1 hand 1 allies 0/1/0 pets 0/23/0 severity 0 alert 0 phase 1 detail NO_ACTIVE_CONDITIONS ttl 6"
    }
    #[test]
    fn bounded_feed_unknown_party_and_expiry() {
        let t = SystemTime::UNIX_EPOCH;
        let a = Adventure::parse(line(), t, t).unwrap();
        assert_eq!(a.pets, [0, 23, 0]);
        assert_eq!(
            parse(&format!("{} ttl 6", a.to_line()), t, t),
            Some(State::Adventure(a))
        );
        for bad in [
            line().replace("0/23/0", "24/23/0"),
            line().replace("0/23/0", "23/23/1"),
            line().replace("blood 100", "blood 101"),
            line().replace("walk 1", "walk 2"),
            format!("{} severity 0", line()),
            line().replace("adv 1", "adv 2"),
        ] {
            assert!(Adventure::parse(&bad, t, t).is_none(), "{bad}");
        }
        assert!(Adventure::parse(line(), t, t + Duration::from_secs(7)).is_none());
        assert_eq!(
            Adventure::parse(&line().replace("blood 100", "blood na"), t, t)
                .unwrap()
                .blood,
            None
        );
        assert_eq!(
            Adventure::parse(&line().replace("0/23/0", "0/23/23"), t, t)
                .unwrap()
                .pets[2],
            23
        );
    }
    #[test]
    fn categorical_colours_flash_cooldown_and_recovery() {
        let t = SystemTime::UNIX_EPOCH;
        let mut a = Adventure::parse(line(), t, t).unwrap();
        let mut m = Meter::default();
        let now = Instant::now();
        for (severity, name) in [
            (0, "green"),
            (1, "yellow"),
            (2, "orange"),
            (3, "red"),
            (4, "dead"),
        ] {
            a.severity = severity;
            m.frame_at(State::Adventure(a), now);
            assert_eq!(
                m.colour(State::Adventure(a), [1, 2, 3]),
                m.tuning.bands.iter().find(|b| b.name == name).unwrap().rgb
            );
        }
        a.severity = 2;
        a.alert = 1;
        let filled = |b: &Bitmap| (0..43).all(|y| (0..160).all(|x| b.get(x, y)));
        assert!(filled(
            &m.frame_at(State::Adventure(a), now + Duration::from_secs(1))
        ));
        a.alert = 2;
        assert!(!filled(
            &m.frame_at(State::Adventure(a), now + Duration::from_secs(2))
        ));
        assert_eq!(
            m.colour(State::Adventure(a), [1, 2, 3]),
            a.colour(&m.tuning)
        );
        a.alert = 3;
        assert!(filled(
            &m.frame_at(State::Adventure(a), now + Duration::from_secs(7))
        ));
        a.session += 1;
        a.unit += 1;
        assert!(!filled(
            &m.frame_at(State::Adventure(a), now + Duration::from_secs(8))
        ));
        assert_eq!(State::Adventure(a).pct(), None);
    }

    #[test]
    #[ignore = "writes LCD review frames when G13MAP_ADVENTURE_DUMP is set"]
    fn dump_adventure_frames() {
        let Some(dir) = std::env::var_os("G13MAP_ADVENTURE_DUMP") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t = SystemTime::UNIX_EPOCH;
        let mut a = Adventure::parse(line(), t, t).unwrap();
        for (condition, effort, severity, detail) in [
            ("READY", "RESTED", 0, "NO_ACTIVE_CONDITIONS"),
            ("EXHAUSTED", "EXHAUSTED", 2, "YOU_-_EXHAUSTED"),
            (
                "INFECTION",
                "TIRED",
                2,
                "LEFT_HAND_-_BLEEDING_-_LEFT_FOOT_-_IMPAIRED",
            ),
            ("SUFFOCATING", "RESTED", 3, "YOU_-_SUFFOCATING"),
            (
                "READY",
                "RESTED",
                0,
                "PARTY_STATUS_UNKNOWN_-_1_ALLIES_-_3_PETS",
            ),
        ] {
            a.condition = Label::parse(condition).unwrap();
            a.effort = Label::parse(effort).unwrap();
            a.name = Label::parse("GUKI_EKOPUJA_POLISHBENT_PIKEMAN").unwrap();
            a.detail = Label::parse(detail).unwrap();
            a.severity = severity;
            if detail.starts_with("PARTY") {
                a.allies = [0, 1, 1];
                a.pets = [0, 23, 3];
            }
            let b = Meter::default().frame(State::Adventure(a));
            let mut pbm = String::from("P1\n160 43\n");
            for y in 0..43 {
                for x in 0..160 {
                    pbm.push_str(if b.get(x, y) { "1 " } else { "0 " });
                }
                pbm.push('\n');
            }
            std::fs::write(dir.join(format!("{condition}-{detail}.pbm")), pbm).unwrap();
        }
    }
}
