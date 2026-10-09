// SPDX-License-Identifier: GPL-3.0-or-later
//! Live Adventure travel needs; local health is unavailable while units unload.
use super::{fortress, text, Label, Tuning};
use crate::draw::Draw;
use crate::lcd::{Bitmap, W};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Travel {
    pub session: u64,
    pub unit: u32,
    pub name: Label,
    pub needs: [u8; 3], // food, water, sleep: 0 okay, 1 due, 2 not needed
    pub activity: Label,
    pub allies: u32,
    pub pets: u32,
    pub phase: u64,
    pub detail: Label,
}
impl Travel {
    pub(super) fn parse(line: &str, written: SystemTime, now: SystemTime) -> Option<Self> {
        if line.len() > 1536 {
            return None;
        }
        let mut words = line.split_whitespace();
        if words.next()? != "travel" || words.next()? != "1" {
            return None;
        }
        const KEYS: [&str; 12] = [
            "session", "unit", "name", "food", "water", "rest", "activity", "allies", "pets",
            "phase", "detail", "ttl",
        ];
        let mut values = [None; 12];
        while let Some(key) = words.next() {
            let i = KEYS.iter().position(|k| *k == key)?;
            if values[i].is_some() {
                return None;
            }
            values[i] = Some(words.next()?);
        }
        let get = |i: usize| values[i];
        let ttl = get(11)?
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=60).contains(n))?;
        if now.duration_since(written).unwrap_or_default() > Duration::from_secs(ttl.into()) {
            return None;
        }
        let need = |i| get(i)?.parse::<u8>().ok().filter(|n| *n <= 2);
        let count = |i| get(i)?.parse::<u32>().ok().filter(|n| *n <= 10000);
        let activity = get(6)?;
        if ![
            "WALKING",
            "SNEAKING",
            "SLEEPING",
            "WAITING",
            "COMPOSING",
            "WORKING",
            "ON_WATCH",
        ]
        .contains(&activity)
        {
            return None;
        }
        Some(Self {
            session: get(0)?.parse().ok()?,
            unit: get(1)?
                .parse::<u32>()
                .ok()
                .filter(|n| *n <= i32::MAX as u32)?,
            name: Label::parse(get(2)?)?,
            needs: [need(3)?, need(4)?, need(5)?],
            activity: Label::parse(activity)?,
            allies: count(7)?,
            pets: count(8)?,
            phase: get(9)?.parse().ok()?,
            detail: Label::parse(get(10)?)?,
        })
    }
    pub(super) fn to_line(self) -> String {
        format!("travel 1 session {} unit {} name {} food {} water {} rest {} activity {} allies {} pets {} phase {} detail {}",
            self.session, self.unit, self.name.as_str(), self.needs[0], self.needs[1], self.needs[2],
            self.activity.as_str(), self.allies, self.pets, self.phase, self.detail.as_str())
    }
    pub(super) fn colour(self, tuning: &Tuning) -> [u8; 3] {
        // Blue signals travel with unavailable health; green would imply all clear.
        let (name, fallback) = if self.needs.contains(&1) {
            ("yellow", [255, 215, 0])
        } else {
            ("blue", [0, 160, 255])
        };
        tuning
            .bands
            .iter()
            .find(|b| b.name == name)
            .map_or(fallback, |b| b.rgb)
    }
    pub(super) fn draw(self, display: &mut fortress::Display, bm: &mut Bitmap, now: Instant) {
        display.begin(self.session, 0, 0, now);
        bm.sprite(0, 0, fortress::DWARF);
        let name: String = self
            .name
            .as_str()
            .split('_')
            .next()
            .unwrap_or("")
            .chars()
            .take(15)
            .collect();
        text(bm, 12, 1, &name, 1);
        text(bm, 124, 1, "TRAVEL", 1);
        let need = |n| match n {
            0 => "OK",
            1 => "DUE",
            _ => "--",
        };
        text(bm, 0, 10, &format!("FOOD {}", need(self.needs[0])), 1);
        text(bm, 78, 10, &format!("WATER {}", need(self.needs[1])), 1);
        text(bm, 0, 18, &format!("SLEEP {}", need(self.needs[2])), 1);
        text(bm, 78, 18, self.activity.as_str(), 1);
        let count = |v| {
            if v < 100 {
                format!("{v}")
            } else {
                "99+".into()
            }
        };
        text(bm, 0, 26, &format!("ALLY {}", count(self.allies)), 1);
        text(bm, 78, 26, &format!("PETS {}", count(self.pets)), 1);
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
        "travel 1 session 20 unit 1309 name ELANA food 0 water 0 rest 0 activity WALKING allies 3 pets 4 phase 1 detail HEALTH_UNAVAILABLE_WHILE_TRAVELLING ttl 6"
    }
    #[test]
    fn travel_bounds_expiry_and_roundtrip() {
        let t = SystemTime::UNIX_EPOCH;
        let a = Travel::parse(line(), t, t).unwrap();
        assert_eq!(
            parse(&format!("{} ttl 6", a.to_line()), t, t),
            Some(State::Travel(a))
        );
        for bad in [
            line().replace("food 0", "food 3"),
            line().replace("rest 0", "rest -1"),
            line().replace("allies 3", "allies 10001"),
            line().replace("WALKING", "READY"),
            line().replace("travel 1", "travel 2"),
            format!("{} blood 100", line()),
            format!("{} food 0", line()),
            line().replace(" ttl 6", ""),
            line().replace("unit 1309", "unit 2147483648"),
        ] {
            assert!(Travel::parse(&bad, t, t).is_none(), "{bad}");
        }
        assert!(Travel::parse(line(), t, t + Duration::from_secs(7)).is_none());
    }
    #[test]
    fn travel_neutral_colour_and_mode_transition() {
        let t = SystemTime::UNIX_EPOCH;
        let mut a = Travel::parse(line(), t, t).unwrap();
        let mut m = Meter::default();
        let now = Instant::now();
        m.frame_at(State::health(0), now);
        m.frame_at(State::Travel(a), now);
        let blue = m
            .tuning
            .bands
            .iter()
            .find(|b| b.name == "blue")
            .unwrap()
            .rgb;
        assert_eq!(m.colour(State::Travel(a), [1, 2, 3]), blue);
        assert_eq!(State::Travel(a).pct(), None);
        a.needs = [1, 2, 0];
        m.frame_at(State::Travel(a), now + Duration::from_secs(1));
        let yellow = m
            .tuning
            .bands
            .iter()
            .find(|b| b.name == "yellow")
            .unwrap()
            .rgb;
        assert_eq!(m.colour(State::Travel(a), [1, 2, 3]), yellow);
        a.session += 1;
        a.unit += 1;
        assert!(!m
            .frame_at(State::Travel(a), now + Duration::from_secs(2))
            .0
            .iter()
            .all(|b| *b == 255));
    }
    #[test]
    #[ignore = "writes travel LCD review frames when G13MAP_TRAVEL_DUMP is set"]
    fn dump_travel_frames() {
        let Some(dir) = std::env::var_os("G13MAP_TRAVEL_DUMP") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t = SystemTime::UNIX_EPOCH;
        let mut a = Travel::parse(line(), t, t).unwrap();
        for (name, needs, activity) in [
            ("travel", [0, 0, 0], "WALKING"),
            ("needs", [1, 1, 1], "SNEAKING"),
            ("sleep", [2, 2, 2], "SLEEPING"),
        ] {
            a.needs = needs;
            a.activity = Label::parse(activity).unwrap();
            let b = Meter::default().frame(State::Travel(a));
            let mut pbm = String::from("P1\n160 43\n");
            for y in 0..43 {
                for x in 0..160 {
                    pbm.push_str(if b.get(x, y) { "1 " } else { "0 " });
                }
                pbm.push('\n');
            }
            std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
        }
    }
}
