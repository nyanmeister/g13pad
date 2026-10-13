// SPDX-License-Identifier: GPL-3.0-or-later
//! Render responsibilities for Health Mode.
use super::*;

// ---- the picture ----

/// Frames come every tick; the trace scrolls this many columns per tick.
/// Legacy heartbeat logical step. Independent of collector and event-frame cadence.
pub const TICK: Duration = Duration::from_millis(100);
pub(super) const SCROLL: i32 = 5;
pub(super) const BASE: i32 = 22;

#[rustfmt::skip]
pub(super) const HEART: &[&str] = &[
    ".##.##.", "#######", "#######", ".#####.", "..###..", "...#...",
];
#[rustfmt::skip]
pub(super) const HEART_BIG: &[&str] = &[
    ".###.###.",
    "#########",
    "#########",
    "#########",
    ".#######.",
    "..#####..",
    "...###...",
    "....#....",
];
#[rustfmt::skip]
pub(super) const HEART_OUTLINE: &[&str] = &[
    ".##.##.", "#..#..#", "#.....#", ".#...#.", "..#.#..", "...#...",
];

/// V1, ULTRAKILL's machine, 15 by 13: the fins, the visor, the jaw. Blinks on the beat.
#[rustfmt::skip]
pub(super) const V1: &[&str] = &[
    "#.....###.....#",
    "##...#####...##",
    "###.#######.###",
    "####.#####.####",
    ".#####...#####.",
    "..###########..",
    "..#.........#..",
    "..#..#####..#..",
    "..#.........#..",
    "...#########...",
    "....#.....#....",
    "....##...##....",
    ".....#####.....",
];
#[rustfmt::skip]
pub(super) const V1_BLINK: &[&str] = &[
    "#.....###.....#",
    "##...#####...##",
    "###.#######.###",
    "####.#####.####",
    ".#####...#####.",
    "..###########..",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "...#########...",
    "....#.....#....",
    "....##...##....",
    ".....#####.....",
];
#[rustfmt::skip]
pub(super) const V1_OUTLINE: &[&str] = &[
    "#.....###.....#",
    "##...#...#...##",
    "#.#.#.....#.#.#",
    "#..#.......#..#",
    ".#...........#.",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "...#.......#...",
    "....#.....#....",
    "....#.....#....",
    ".....#####.....",
];

/// Letters the feed may put on the panel, including arbitrary research names.
#[rustfmt::skip]
pub(super) const LETTERS: [(char, [&str; 7]); 37] = [
    ('A', [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('B', ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."]),
    ('C', [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."]),
    ('D', ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."]),
    ('S', [".####", "#....", "#....", ".###.", "....#", "....#", "####."]),
    ('U', ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    (',', [".....", ".....", ".....", ".....", "..#..", "..#..", ".#..."]),
    (':', [".....", "..#..", "..#..", ".....", "..#..", "..#..", "....."]),
    ('.', [".....", ".....", ".....", ".....", ".....", "..#..", "..#.."]),
    ('M', ["#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#"]),
    ('P', ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."]),
    ('E', ["#####", "#....", "#....", "####.", "#....", "#....", "#####"]),
    ('F', ["#####", "#....", "#....", "####.", "#....", "#....", "#...."]),
    ('I', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"]),
    ('R', ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"]),
    ('G', [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###."]),
    ('H', ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('J', ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."]),
    ('K', ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"]),
    ('L', ["#....", "#....", "#....", "#....", "#....", "#....", "#####"]),
    ('N', ["#...#", "##..#", "##..#", "#.#.#", "#..##", "#..##", "#...#"]),
    ('O', [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('Q', [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"]),
    ('T', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."]),
    ('V', ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."]),
    ('W', ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"]),
    ('X', ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"]),
    ('Y', ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."]),
    ('Z', ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"]),
    ('/', ["....#", "....#", "...#.", "..#..", ".#...", "#....", "#...."]),
    (' ', [".....", ".....", ".....", ".....", ".....", ".....", "....."]),
    ('%', ["##..#", "##..#", "...#.", "..#..", ".#...", "#..##", "#..##"]),
    ('(', ["...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#."]),
    (')', [".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#..."]),
    ('°', [".###.", ".#.#.", ".###.", ".....", ".....", ".....", "....."]),
    ('?', [".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#.."]),
    ('+', [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."]),
];

/// The 5x7 glyph for `c`: a digit, a letter from `LETTERS`, or the dash.
pub(super) fn glyph(c: char) -> &'static [&'static str; 7] {
    if let Some(d) = c.to_digit(10) {
        return &GLYPHS[d as usize];
    }
    LETTERS
        .iter()
        .find(|(k, _)| *k == c)
        .map_or(&GLYPHS[10], |(_, g)| g)
}

/// `text` in 5x7 glyphs at `scale`, top-left at `x, y`; returns the width drawn.
pub(crate) fn text(bm: &mut Bitmap, x: i32, y: i32, text: &str, scale: i32) -> i32 {
    for (i, c) in text.chars().enumerate() {
        for (r, row) in glyph(c).iter().enumerate() {
            for (k, cell) in row.bytes().enumerate() {
                if cell == b'#' {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            bm.plot(
                                x + (i as i32 * 6 + k as i32) * scale + dx,
                                y + r as i32 * scale + dy,
                            );
                        }
                    }
                }
            }
        }
    }
    (text.chars().count() as i32 * 6 - 1) * scale
}

/// The level timer as `m:ss`, or `h:mm:ss` past an hour (a hard boss can take three).
pub(super) fn clock(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 5x7 digits and a dash, drawn at twice the size for the readout.
#[rustfmt::skip]
pub(super) const GLYPHS: [[&str; 7]; 11] = [
    [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
    ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
    [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
    ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
    ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
    ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
    ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
    ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
    [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
    [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
    [".....", ".....", ".....", "#####", ".....", ".....", "....."],
];

/// The readout, right-aligned to `right`, 2x glyphs, on a darkened patch.
pub(super) fn readout(bm: &mut Bitmap, text: &str, right: i32, y: i32) {
    let w = text.len() as i32 * 12 - 2;
    let x0 = right - w;
    bm.halo(x0, y, w, 14);
    self::text(bm, x0, y, text, 2);
}

/// A small gauge: a box `w` by `h` with `fill` (0–100) of its inside lit.
pub(super) fn gauge(bm: &mut Bitmap, x: i32, y: i32, w: i32, h: i32, fill: u32) {
    bm.halo(x, y, w, h);
    bm.line(x, y, x + w - 1, y);
    bm.line(x, y + h - 1, x + w - 1, y + h - 1);
    bm.line(x, y, x, y + h - 1);
    bm.line(x + w - 1, y, x + w - 1, y + h - 1);
    let f = ((w - 4) * fill.min(100) as i32 + 50) / 100;
    for xx in x + 2..x + 2 + f {
        for yy in y + 2..y + h - 2 {
            bm.plot(xx, yy);
        }
    }
}

/// Eight teeth and three cutouts remain legible in the small vehicle icon.
/// Cracks rotate with the metal, rather than flickering with the heartbeat.
pub(super) fn gear(bm: &mut Bitmap, cx: i32, cy: i32, angle: f32, damaged: bool) {
    use std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    for dy in -6..=6 {
        for dx in -6..=6 {
            let radius = ((dx * dx + dy * dy) as f32).sqrt();
            let theta = (dy as f32).atan2(dx as f32) - angle;
            let tooth = (theta + TAU / 16.0).rem_euclid(TAU / 8.0) - TAU / 16.0;
            let edge = 4.4 + 1.6 * (1.0 - (tooth.abs() - 0.11).max(0.0) / 0.13).max(0.0);
            let hole = (theta + TAU / 6.0).rem_euclid(TAU / 3.0) - TAU / 6.0;
            let along = dx as f32 * cos + dy as f32 * sin;
            let across = (-dx as f32 * sin + dy as f32 * cos) * along.signum();
            let bend = if (2.6..4.1).contains(&along.abs()) {
                1.0
            } else {
                0.0
            };
            // A two-pixel-wide zigzag splits the metal all the way through its
            // rim. Mask this wheel before compositing, so a crack cannot erase
            // a neighbouring wheel at the interlocking teeth.
            let crack = damaged && along.abs() > 1.0 && (across - bend).abs() <= 1.0;
            if radius > 1.25
                && radius <= edge
                && !(radius > 2.0 && radius < 3.8 && hole.abs() < 0.22)
                && !crack
            {
                bm.plot(cx + dx, cy + dy);
            }
        }
    }
}

pub(super) const BAR_X: i32 = 4;
pub(super) const BAR_W: i32 = 152;

/// The shield bar's top row; its second hundred adds a row above and below.
pub(super) const SHIELD_TOP: i32 = 30;

/// The ring around a sprite `w` by `h` at `hx, hy`, `out` pixels further out than the
/// first, with its corners cut.
pub(super) fn ring(bm: &mut Bitmap, hx: i32, hy: i32, w: i32, h: i32, out: i32) {
    let (l, t, r, b) = (
        hx - 4 - out,
        hy - 3 - out,
        hx + w + 3 + out,
        hy + h + 2 + out,
    );
    bm.line(l + 1, t, r - 1, t);
    bm.line(l + 1, b, r - 1, b);
    bm.line(l, t + 1, l, b - 1);
    bm.line(r, t + 1, r, b - 1);
}

/// The health bar along the bottom with notches at the band edges, and the shield bar
/// above it when there is any shield: hatched, or solid when the head is covered too.
/// `shield` may run to twice the bar: the second hundred thickens it from the left.
/// `cap` hatches the bar from there to full: health the game has taken away for now.
pub(super) fn bars(
    bm: &mut Bitmap,
    fill: i32,
    shield: i32,
    solid: bool,
    notches: &[i32],
    cap: Option<i32>,
) {
    if let Some(cap) = cap {
        for x in BAR_X + cap.clamp(0, BAR_W)..BAR_X + BAR_W {
            for y in 36..=40 {
                if (x + y) % 2 == 0 {
                    bm.plot(x, y);
                }
            }
        }
    }
    for x in 2..=157 {
        bm.plot(x, 34);
        bm.plot(x, 42);
    }
    for y in 34..=42 {
        bm.plot(2, y);
        bm.plot(157, y);
    }
    for &x in notches {
        bm.put(x, 34, false);
        bm.put(x, 42, false);
    }
    for x in BAR_X..BAR_X + fill.clamp(0, BAR_W) {
        for y in 36..=40 {
            bm.plot(x, y);
        }
    }
    let hatch = |bm: &mut Bitmap, x: i32, y: i32| {
        if solid || (x + y) % 2 == 0 {
            bm.plot(x, y);
        }
    };
    for x in BAR_X..BAR_X + shield.clamp(0, BAR_W) {
        for y in SHIELD_TOP..=SHIELD_TOP + 2 {
            hatch(bm, x, y);
        }
    }
    for x in BAR_X..BAR_X + (shield - BAR_W).clamp(0, BAR_W) {
        hatch(bm, x, SHIELD_TOP - 1);
        hatch(bm, x, SHIELD_TOP + 3);
    }
}

/// The meter's picture as it runs: the trace as a ring of column lifts, scrolled left
/// `SCROLL` columns a tick with new columns from the beat clock on the right. A pure
/// function of the ticks and states it was given, so tests can draw it without a clock.
pub struct Meter {
    pub(super) fortress: fortress::Display,
    pub(super) attack_flash: Option<Instant>,
    pub(super) beat: parts::Heartbeat,
    pub(super) hold: parts::Hold,
    pub(super) tick: u32,
    pub(super) research_name: Label,
    pub(super) research_started: u32,
    /// The look; the frame thread takes a new one from the file.
    pub tuning: Tuning,
}

impl Default for Meter {
    fn default() -> Self {
        Meter {
            fortress: fortress::Display::default(),
            attack_flash: None,
            beat: parts::Heartbeat::default(),
            hold: parts::Hold::default(),
            tick: 0,
            research_name: Label::EMPTY,
            research_started: 0,
            tuning: Tuning::default(),
        }
    }
}

impl Meter {
    /// Advances one tick in `state` and draws it.
    pub fn frame(&mut self, state: State) -> Bitmap {
        self.frame_at(state, Instant::now())
    }

    pub(super) fn frame_at(&mut self, state: State, now: Instant) -> Bitmap {
        let mut bm = Bitmap::blank();
        self.tick = self.tick.wrapping_add(1);
        if let State::Fortress(fort) = state {
            self.hold = parts::Hold::default();
            self.fortress.draw(&mut bm, fort, now);
            return bm;
        }
        if let State::Adventure(adv) = state {
            self.hold = parts::Hold::default();
            adv.draw(&mut self.fortress, &mut bm, now);
            return bm;
        }
        if let State::Travel(travel) = state {
            self.hold = parts::Hold::default();
            travel.draw(&mut self.fortress, &mut bm, now);
            return bm;
        }
        // A drop to zero always holds the dark flatline for the whole hold, whatever the
        // feed says meanwhile (asked 2026-10-07); only then does the next state show.
        let zero =
            matches!(state, State::Health { pct: 0, extra, .. } if extra.vehicle == Label::EMPTY);
        self.hold.tick(zero, self.tuning.hold_ticks());
        match state {
            _ if self.holding() => self.alive(&mut bm, 0, 0, false, Extra::NONE),
            State::Fortress(fort) => self.fortress.draw(&mut bm, fort, now),
            State::Adventure(adv) => adv.draw(&mut self.fortress, &mut bm, now),
            State::Travel(travel) => travel.draw(&mut self.fortress, &mut bm, now),
            State::Wait => self.waiting(&mut bm),
            State::Health {
                pct, shield, extra, ..
            } if extra.vehicle != Label::EMPTY => {
                self.vehicle(&mut bm, pct, shield, extra);
            }
            State::Health { pct: 0, .. } => self.waiting(&mut bm),
            State::Health {
                pct,
                shield,
                helmet,
                extra,
            } => self.alive(&mut bm, pct, shield, helmet, extra),
        }
        if let State::Health { extra, .. } = state {
            if extra.attack.unwrap_or(0) > 0 && !self.holding() {
                if self
                    .attack_flash
                    .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(6))
                {
                    self.attack_flash = Some(now);
                }
                if self
                    .attack_flash
                    .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(300))
                {
                    // Fill the entire visible glass, including the bars. A steady label
                    // remains between flashes; changing alert counts cannot retrigger it.
                    for y in 0..H {
                        for x in 0..W {
                            bm.set(x, y, true);
                        }
                    }
                }
            }
        }
        bm
    }

    /// The corner sprite and its size: the heart, or V1.
    pub(super) fn sprite(&self, pct: u32) -> (&'static [&'static str], i32, i32, i32, i32) {
        let swell = self.beat.swelling(self.tuning.swell_ticks());
        match self.tuning.sprite {
            Sprite::Heart => match (pct, swell) {
                (0, _) => (HEART_OUTLINE, 7, 6, 0, 0),
                (_, true) => (HEART_BIG, 7, 6, -1, -1),
                _ => (HEART, 7, 6, 0, 0),
            },
            Sprite::V1 => match (pct, swell) {
                (0, _) => (V1_OUTLINE, 15, 13, 0, 0),
                (_, true) => (V1_BLINK, 15, 13, 0, 0),
                _ => (V1, 15, 13, 0, 0),
            },
        }
    }

    /// Inside the dark seconds after a drop to zero.
    pub(super) fn holding(&self) -> bool {
        self.hold.holding(self.tuning.hold_ticks())
    }

    /// The backlight for the frame just drawn: off through the hold, the band's colour,
    /// the resting colour while waiting, red once a flatline has turned into the search
    /// for a pulse.
    pub fn colour(&self, state: State, rest: [u8; 3]) -> [u8; 3] {
        if let State::Travel(travel) = state {
            return travel.colour(&self.tuning);
        }
        if let State::Adventure(adv) = state {
            return if adv.severity != 4 && self.fortress.is_flashing() {
                self.tuning.alarm().rgb
            } else {
                adv.colour(&self.tuning)
            };
        }
        if let State::Fortress(fort) = state {
            if let Some(season) = fort.season {
                return if self.fortress.is_flashing() {
                    self.tuning.alarm().rgb
                } else {
                    season.rgb()
                };
            }
            return match fort.severity {
                2 => self.tuning.alarm().rgb,
                1 => self.tuning.rgb_at(60),
                _ => rest,
            };
        }
        if self.holding() {
            return self.tuning.rgb_at(0);
        }
        match state.pct() {
            None => rest,
            Some(0) => self.tuning.alarm().rgb,
            Some(pct) => self.tuning.rgb_at(pct),
        }
    }

    /// One tick of the beat at `pct`, at the tuning's pace.
    pub(super) fn scroll(&mut self, pct: u32) {
        let period = self.tuning.period(pct);
        self.beat.tick(pct, period);
    }

    pub(super) fn alive(
        &mut self,
        bm: &mut Bitmap,
        pct: u32,
        shield: u32,
        helmet: bool,
        extra: Extra,
    ) {
        self.scroll(pct);
        self.beat.trace(bm);
        let [hx, hy] = self.tuning.heart;
        let (sprite, w, h, dx, dy) = self.sprite(pct);
        if self.tuning.sprite == Sprite::V1 {
            bm.halo(hx, hy, w, h);
        }
        bm.sprite(hx + dx, hy + dy, sprite);
        if shield > 0 {
            // Rings around the heart, the shield's own mark: one per hundred, up to three
            // (Doom's blue armour is 200 and mods go past it; asked 2026-10-08).
            for out in 0..shield.div_ceil(100).min(3) {
                ring(bm, hx, hy, w, h, out as i32);
            }
        }
        self.extras_by_the_sprite(bm, hx, hy, h, extra);
        let [rx, ry] = self.tuning.readout;
        readout(bm, &pct.min(999).to_string(), rx, ry);
        self.extras_on_the_panel(bm, rx, ry, extra);
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            (BAR_W * shield.min(200) as i32 + 50) / 100,
            helmet,
            &self.tuning.notches(),
            extra.cap.map(|c| (BAR_W * c.min(100) as i32 + 50) / 100),
        );
        // In the alarm band every beat flashes: the panel, or the trace alone.
        if self.beat.beat_now() && self.tuning.is_alarm(pct) {
            let rows = match self.tuning.flash {
                Flash::Off => 0,
                Flash::Trace => SHIELD_TOP as usize - 1,
                Flash::Panel => H,
            };
            for y in 0..rows {
                for x in 0..W {
                    bm.set(x, y, !bm.get(x, y));
                }
            }
        }
    }

    /// What a game adds beside its sprite and at the top centre, drawn before the
    /// readout: ULTRAKILL's dashes and rail charge, its style rank and meter.
    fn extras_by_the_sprite(&mut self, bm: &mut Bitmap, hx: i32, hy: i32, h: i32, extra: Extra) {
        // Under the sprite: the dashes as pips, a charge as a small gauge.
        let under = hy + h + 2;
        if let Some(d) = extra.dash {
            bm.halo(hx - 3, under, 16, 4);
            for i in 0..3u8 {
                let x = hx - 3 + i as i32 * 6;
                let rows = match d.saturating_sub(i * 10) {
                    10.. => 4,
                    1..=9 => 2,
                    0 => 1,
                };
                for y in under..under + rows {
                    bm.line(x, y, x + 3, y);
                }
            }
        }
        if let Some(r) = extra.rail {
            gauge(bm, hx + 16, under - 1, 20, 6, r as u32);
        }
        // The style rank, its letters big at the top centre with its own meter beneath.
        let [_, ry] = self.tuning.readout;
        if let Some(rank) = extra.rank {
            let letters = RANKS[rank as usize % RANKS.len()];
            let w = (letters.len() as i32 * 6 - 1) * 2;
            bm.halo(64, ry, w, 14);
            text(bm, 64, ry, letters, 2);
            if let Some(fill) = extra.style {
                gauge(bm, 64, ry + 15, w.max(20), 5, fill as u32);
            }
        }
    }

    /// What a game adds on the panel after the readout: the level timer, mana, defence,
    /// air, the suit battery, attacks or research.
    fn extras_on_the_panel(&mut self, bm: &mut Bitmap, rx: i32, ry: i32, extra: Extra) {
        if let Some(t) = extra.time {
            let s = clock(t);
            let w = s.len() as i32 * 6 - 1;
            bm.halo(rx - w, ry + 16, w, 7);
            text(bm, rx - w, ry + 16, &s, 1);
        }
        if let Some(mana) = extra.mana {
            let label = format!("MP {}/{}", mana.current, mana.maximum);
            // Keep large modded pools inside this slot without obscuring health.
            let label = if label.len() > 11 {
                format!("MP {}%", mana.percent())
            } else {
                label
            };
            bm.halo(43, 2, 65, 14);
            text(bm, 43, 2, &label, 1);
            gauge(bm, 43, 11, 65, 6, mana.percent());
        }
        if let Some(defense) = extra.defense {
            bm.halo(43, 19, 41, 7);
            text(bm, 43, 19, &format!("DEF {defense}"), 1);
        }
        if let Some(breath) = extra.breath.filter(|b| b.current < b.maximum) {
            bm.halo(88, 19, 50, 7);
            text(bm, 88, 19, "AIR", 1);
            gauge(bm, 109, 20, 29, 5, breath.percent());
        }
        if let Some(battery) = extra.battery {
            bm.halo(43, 2, 65, 15);
            text(bm, 43, 2, &format!("BAT {battery}%"), 1);
            gauge(bm, 43, 11, 65, 6, battery.into());
        }
        if let Some(attacks) = extra.attack.filter(|n| *n > 0) {
            bm.halo(43, 19, 95, 7);
            text(bm, 43, 19, &format!("ATTACK {attacks}"), 1);
        } else if let Some(research) = extra.research {
            bm.halo(43, 19, 95, 7);
            self.research(bm, 43, 19, extra.technology, research);
        }
    }

    pub(super) fn vehicle(&mut self, bm: &mut Bitmap, pct: u32, shield: u32, extra: Extra) {
        // Returning on foot starts a fresh ECG instead of retaining old beats.
        self.beat.reset();
        let phase = (self.tick % 48) as i32;
        match self.tuning.vehicle {
            VehicleStyle::Tread => {
                // A steady hull with circulating tread links, independent of HP.
                bm.line(5, 9, 25, 9);
                bm.line(5, 16, 25, 16);
                bm.line(5, 9, 3, 12);
                bm.line(3, 12, 5, 16);
                bm.line(25, 9, 27, 12);
                bm.line(27, 12, 25, 16);
                bm.line(8, 7, 22, 7);
                bm.line(8, 7, 8, 9);
                bm.line(22, 7, 22, 9);
                bm.line(13, 4, 19, 4);
                bm.line(13, 4, 13, 7);
                bm.line(19, 4, 19, 7);
                bm.line(19, 5, 27, 5);
                for x in 5..=25 {
                    if (x + phase / 2) % 4 < 2 {
                        bm.plot(x, 11);
                        bm.plot(30 - x, 14);
                    }
                }
            }
            VehicleStyle::Gear => {
                let angle = phase as f32 * std::f32::consts::TAU / 48.0;
                let damaged = self.tuning.vehicle_damaged(pct);
                // Adjacent wheels counter-rotate; the middle meshes with both.
                gear(bm, 7, 6, angle, damaged);
                gear(bm, 16, 12, -angle, damaged);
                gear(bm, 25, 6, angle, damaged);
            }
            VehicleStyle::Scan => {
                gauge(bm, 3, 2, 26, 18, 0);
                let step = phase % 48;
                let x = 4 + if step < 24 { step } else { 47 - step };
                bm.line(x, 5, x, 16);
                bm.line(x - 1, 7, x - 1, 14);
            }
        }
        let label = extra
            .vehicle
            .as_str()
            .replace(['-', '_'], " ")
            .to_uppercase();
        text(bm, 33, 2, &format!("{label:.13}"), 1);
        readout(bm, &pct.min(999).to_string(), 157, 2);
        if let Some(pilot) = extra.pilot {
            text(bm, 33, 12, &format!("HP {pilot}%"), 1);
        }
        text(bm, 3, 23, &format!("SH {shield}%"), 1);
        if let Some(battery) = extra.battery {
            text(bm, 75, 12, &format!("BAT {battery}%"), 1);
        }
        if let Some(attacks) = extra.attack.filter(|n| *n > 0) {
            text(bm, 55, 23, &format!("ATTACK {attacks}"), 1);
        } else if let Some(research) = extra.research {
            self.research(bm, 55, 23, extra.technology, research);
        }
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            0,
            false,
            &self.tuning.notches(),
            None,
        );
    }

    pub(super) fn research(&mut self, bm: &mut Bitmap, x: i32, y: i32, label: Label, progress: u8) {
        if label != self.research_name {
            self.research_name = label;
            self.research_started = self.tick;
        }
        let name = label.as_str();
        let name = if name.is_empty() { "SCIENCE" } else { name };
        let name = name.replace(['-', '_'], " ").to_uppercase();
        const WIDTH: i32 = 53; // nine small glyphs, with progress in a separate slot
        let length = name.len() as i32 * 6 - 1;
        if length <= WIDTH {
            text(bm, x, y, &name, 1);
        } else {
            let loop_width = length + 19; // three spaces between copies
            let cycle = 12 + (loop_width as u32).div_ceil(2);
            let phase = self.tick.wrapping_sub(self.research_started) % cycle;
            let offset = phase.saturating_sub(12) as i32 * 2;
            let mut clipped = Bitmap::blank();
            text(&mut clipped, -offset, 0, &name, 1);
            text(&mut clipped, loop_width - offset, 0, &name, 1);
            for yy in 0..7 {
                for xx in 0..WIDTH {
                    bm.put(x + xx, y + yy, clipped.get(xx as usize, yy as usize));
                }
            }
        }
        text(bm, x + 56, y, &format!("{progress}%"), 1);
    }

    /// Connected, waiting for a pulse: a monitor sweep erasing a flat line ahead of its
    /// cursor, the heart as an outline that fills for a moment now and then, a dashed
    /// readout and a mark pacing the empty bar.
    pub(super) fn waiting(&mut self, bm: &mut Bitmap) {
        self.scroll(0);
        let t = self.tick as i32;
        let cursor = (t * SCROLL) % (W as i32 + 16);
        for x in 0..W as i32 {
            if x < cursor - 12 || x > cursor {
                bm.plot(x, BASE);
            }
        }
        bm.line(cursor, BASE - 2, cursor, BASE + 2);
        let [hx, hy] = self.tuning.heart;
        let full = t % 20 < 3;
        match self.tuning.sprite {
            Sprite::Heart => bm.sprite(hx, hy, if full { HEART } else { HEART_OUTLINE }),
            Sprite::V1 => {
                bm.halo(hx, hy, 15, 13);
                bm.sprite(hx, hy, if full { V1 } else { V1_OUTLINE });
            }
        }
        let [rx, ry] = self.tuning.readout;
        readout(bm, "--", rx, ry);
        bars(bm, 0, 0, false, &self.tuning.notches(), None);
        let x = BAR_X + tri(t * 2, BAR_W - 6);
        for dx in 0..6 {
            for y in 36..=40 {
                bm.plot(x + dx, y);
            }
        }
    }
}

impl Tuning {
    /// Notches on the health bar where each band above the alarm begins, up to full.
    pub(super) fn notches(&self) -> Vec<i32> {
        self.bands
            .iter()
            .filter(|b| b.from > 1 && b.from <= 100)
            .map(|b| BAR_X + BAR_W * (b.from as i32 - 1) / 100)
            .collect()
    }
    pub(super) fn hold_ticks(&self) -> u32 {
        (self.hold / TICK.as_secs_f32()).round().max(1.0) as u32
    }
    pub(super) fn swell_ticks(&self) -> u32 {
        (self.swell / TICK.as_secs_f32()).round().max(1.0) as u32
    }
    /// Columns from one beat to the next at `pct`: the trace runs `SCROLL` columns a
    /// tick, so seconds times columns a second, between racing and calm; never shorter than
    /// the complex itself.
    pub(super) fn period(&self, pct: u32) -> i32 {
        let cols = |secs: f32| (secs * SCROLL as f32 / TICK.as_secs_f32()).round() as i32;
        let (racing, calm) = (cols(self.racing), cols(self.calm));
        (racing + (calm - racing) * pct.min(100) as i32 / 100).max(ECG_COLUMNS)
    }
}
