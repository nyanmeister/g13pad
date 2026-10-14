// SPDX-License-Identifier: GPL-3.0-or-later
//! State responsibilities for Health Mode.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)] // bounded Copy labels; avoid per-frame heap ownership
pub enum State {
    /// Connected, no health yet.
    Wait,
    /// Fortress-wide counts, rather than a player's health percentage.
    Fortress(Fortress),
    /// The controlled adventurer's condition, separate from traditional HP.
    Adventure(Adventure),
    /// Live needs and membership while the local Adventure map is unloaded.
    Travel(Travel),
    /// Health and shield, both in percent (health may exceed 100), whether the head
    /// is covered too (CS2's helmet; asked 2026-10-08): the shield bar is solid then,
    /// and whatever else the game has to show.
    Health {
        pct: u32,
        shield: u32,
        helmet: bool,
        extra: Extra,
    },
}

/// What a game may add to the feed line; each word draws its own element and nothing
/// is drawn for a word that is absent (asked 2026-10-08, for ULTRAKILL's V1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Extra {
    /// Vehicle prototype name: primary health belongs to the vehicle while riding.
    pub vehicle: Label,
    /// Character health, percent, while vehicle health is the primary value.
    pub pilot: Option<u32>,
    /// Factorio suit battery charge, percent.
    pub battery: Option<u8>,
    /// Current research progress and its prototype name (one feed word).
    pub research: Option<u8>,
    pub technology: Label,
    /// Active base-attack alerts. The renderer owns the six-second cooldown.
    pub attack: Option<u32>,
    /// `mana CURRENT/MAX`: the current and effective maximum mana.
    pub mana: Option<Resource>,
    /// `defense VALUE`: damage mitigation, not an armour pool.
    pub defense: Option<u32>,
    /// `breath CURRENT/MAX`: remaining air; shown when below maximum.
    pub breath: Option<Resource>,
    /// `cap C`: the health the bar is capped at, percent (hard damage).
    pub cap: Option<u32>,
    /// `rank R`: a style rank, an index into `RANKS`.
    pub rank: Option<u8>,
    /// `style S`: the rank's own meter, percent.
    pub style: Option<u8>,
    /// `time S`: the level timer, whole seconds.
    pub time: Option<u32>,
    /// `dash D`: dashes in tenths (`dash 2.5` is 25; three dashes is 30).
    pub dash: Option<u8>,
    /// `rail R`: a weapon charge, percent.
    pub rail: Option<u8>,
    /// `deaths N`: a death tally. Its own verb — common across games, drawn consistently.
    pub deaths: Option<u32>,
    /// `count LABEL VALUE[/MAX]`: generic labelled readouts the feeder names (skulltulas,
    /// poes, keys…). Fixed-size so `Extra` stays `Copy`; the feeder owns each meaning.
    pub counts: [Option<Count>; COUNTS],
    /// `foe LABEL CUR/MAX`: the Z-targeted enemy/boss. When present it overrides the panel
    /// with the enemy's HP — a reusable lock-on element for any game that sends it.
    pub foe: Option<Foe>,
}

impl Extra {
    pub const NONE: Extra = Extra {
        vehicle: Label::EMPTY,
        pilot: None,
        battery: None,
        research: None,
        technology: Label::EMPTY,
        attack: None,
        mana: None,
        defense: None,
        breath: None,
        cap: None,
        rank: None,
        style: None,
        time: None,
        dash: None,
        rail: None,
        deaths: None,
        counts: [None; COUNTS],
        foe: None,
    };
}

/// Bounded ASCII label in the Copy feed state. Underscores render as spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label([u8; 256]);
impl Default for Label {
    fn default() -> Self {
        Self::EMPTY
    }
}
impl Label {
    pub const EMPTY: Self = Self([0; 256]);
    pub(super) fn parse(s: &str) -> Option<Self> {
        if s.is_empty()
            || s.len() > 256
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return None;
        }
        let mut label = Self::EMPTY;
        label.0[..s.len()].copy_from_slice(s.as_bytes());
        Some(label)
    }
    pub(super) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0[..self.0.iter().position(|c| *c == 0).unwrap_or(256)])
            .unwrap_or("")
    }
}

/// How many generic counters a feed line may carry; ~3 fit the glass at once.
pub const COUNTS: usize = 4;

/// A generic labelled counter: `count LABEL VALUE[/MAX]`. `max == 0` draws no gauge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Count {
    pub label: CountLabel,
    pub value: u32,
    pub max: u32,
}

/// A short counter label (the glass is narrow). Underscores render as spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountLabel([u8; 10]);
impl Default for CountLabel {
    fn default() -> Self {
        Self::EMPTY
    }
}
impl CountLabel {
    pub const EMPTY: Self = Self([0; 10]);
    pub(super) fn parse(s: &str) -> Option<Self> {
        if s.is_empty()
            || s.len() > 10
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return None;
        }
        let mut l = Self::EMPTY;
        l.0[..s.len()].copy_from_slice(s.as_bytes());
        Some(l)
    }
    pub(super) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0[..self.0.iter().position(|c| *c == 0).unwrap_or(10)])
            .unwrap_or("")
    }
}

/// The Z-targeted enemy/boss for the lock-on override: `foe LABEL CUR/MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Foe {
    pub label: CountLabel,
    pub current: u32,
    pub max: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resource {
    pub current: u32,
    pub maximum: u32,
}

impl Resource {
    pub(super) fn parse(value: &str) -> Option<Self> {
        let (current, maximum) = value.split_once('/')?;
        let current = current.parse::<u32>().ok()?;
        let maximum = maximum.parse::<u32>().ok()?;
        if maximum == 0 && current != 0 {
            return None;
        }
        Some(Self { current, maximum })
    }

    pub(super) fn percent(self) -> u32 {
        if self.maximum == 0 {
            0
        } else {
            ((u64::from(self.current) * 100 + u64::from(self.maximum) / 2)
                / u64::from(self.maximum))
            .min(100) as u32
        }
    }
}

/// The style ranks a feed may name, lowest first (ULTRAKILL's).
pub const RANKS: [&str; 8] = ["D", "C", "B", "A", "S", "SS", "SSS", "U"];

impl State {
    pub fn health(pct: u32) -> State {
        State::Health {
            pct,
            shield: 0,
            helmet: false,
            extra: Extra::NONE,
        }
    }
    /// The health, if there is one to show.
    pub fn pct(self) -> Option<u32> {
        match self {
            State::Wait | State::Fortress(_) | State::Adventure(_) | State::Travel(_) => None,
            State::Health { pct, .. } => Some(pct),
        }
    }
}
