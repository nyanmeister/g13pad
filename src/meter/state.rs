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
    /// `deck AT NEXT ICON...`: a strip of the feeder's own 16x16 icons (a wand's spells, a
    /// weapon rack) in their fixed order, NEXT of them from AT framed as what comes
    /// next; the frame walks along the strip. Drawn at the top centre.
    pub deck: Option<Deck>,
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
        deck: None,
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

/// A strip of up to `DECK_ICONS` 16x16 icons the feeder draws itself, sent as 64 hex
/// digits each (16 rows, two bytes per row, the high bit the leftmost pixel). The
/// `next` icons from `at` are framed together: the cards the next cast will draw. The
/// strip keeps the feeder's order, so the frame moves as the deck is played (asked
/// 2026-10-13, watching two bolts: a frame that stays put says nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deck {
    pub icons: [[u8; 32]; DECK_ICONS],
    pub len: u8,
    pub at: u8,
    pub next: u8,
}

/// How many icons a `deck` may carry; the glass shows `DECK_SHOWN` of them at once, a
/// window that slides to keep the frame in view.
pub const DECK_ICONS: usize = 6;
pub const DECK_SHOWN: usize = 4;

impl Deck {
    /// `AT NEXT` then one to six icons, each exactly 64 hex digits; the framed run must
    /// lie inside the strip (`next` may be 0: nothing framed). Anything else is no deck.
    pub(super) fn parse<'a>(
        at: &str,
        next: &str,
        icons: impl Iterator<Item = &'a str>,
    ) -> Option<Self> {
        let at = at.parse::<u8>().ok()?;
        let next = next.parse::<u8>().ok()?;
        let mut deck = Deck {
            icons: [[0; 32]; DECK_ICONS],
            len: 0,
            at: 0,
            next: 0,
        };
        for hex in icons {
            if deck.len as usize == DECK_ICONS || hex.len() != 64 {
                return None;
            }
            for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
                let s = std::str::from_utf8(pair).ok()?;
                deck.icons[deck.len as usize][i] = u8::from_str_radix(s, 16).ok()?;
            }
            deck.len += 1;
        }
        if deck.len == 0 || at >= deck.len || at.checked_add(next)? > deck.len {
            return None;
        }
        deck.at = at;
        deck.next = next;
        Some(deck)
    }
    /// The first icon on the glass: the strip slides just enough to keep the whole
    /// framed run in view, and no further than the strip's end.
    pub fn window(&self) -> usize {
        let len = self.len as usize;
        let last_framed = self.at as usize + (self.next as usize).max(1);
        last_framed
            .saturating_sub(DECK_SHOWN)
            .min(len.saturating_sub(DECK_SHOWN))
    }
    /// The icons as sprite rows for the drawing kit, one string per row.
    pub(super) fn rows(&self, i: usize) -> [String; 16] {
        std::array::from_fn(|y| {
            let bits = u16::from_be_bytes([self.icons[i][2 * y], self.icons[i][2 * y + 1]]);
            (0..16)
                .map(|x| if bits & (0x8000 >> x) != 0 { '#' } else { ' ' })
                .collect()
        })
    }
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
