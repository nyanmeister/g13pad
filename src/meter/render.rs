// SPDX-License-Identifier: GPL-3.0-or-later
//! Render responsibilities for Health Mode.
use super::*;

// ---- the picture ----

/// Frames come every tick: 50 a second, one trace column each, since 2026-10-13 (the
/// glass takes a frame every 10 ms; 50 is the rate at which one column per frame gives
/// the trace its 50 columns a second). Independent of the feed poll, `POLL`.
pub const TICK: Duration = Duration::from_millis(20);
/// How often the feed files are read: telemetry, not presentation.
pub const POLL: Duration = Duration::from_millis(100);
/// Columns the trace scrolls per tick.
pub(super) const SCROLL: i32 = 1;
/// Ticks in a tenth of a second: the unit the animations were first drawn in, and the
/// length of the alarm flash.
pub(super) const TICKS_PER_100MS: u32 = (100 / TICK.as_millis()) as u32;
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

/// Like `text`, but XORs each glyph pixel against what is already there, so numbers drawn
/// over a filled gauge stay legible on both the lit and unlit parts of the bar.
fn text_xor(bm: &mut Bitmap, x: i32, y: i32, s: &str, scale: i32) {
    for (i, c) in s.chars().enumerate() {
        for (r, row) in glyph(c).iter().enumerate() {
            for (k, cell) in row.bytes().enumerate() {
                if cell == b'#' {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = x + (i as i32 * 6 + k as i32) * scale + dx;
                            let py = y + r as i32 * scale + dy;
                            if px >= 0 && py >= 0 {
                                let (ux, uy) = (px as usize, py as usize);
                                if ux < W && uy < H {
                                    bm.set(ux, uy, !bm.get(ux, uy));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
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

/// The deck strip: where the first icon sits and the pitch between icons.
pub(super) const DECK_X: i32 = 44;
pub(super) const DECK_Y: i32 = 1;
pub(super) const DECK_STEP: i32 = 18;
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
        // The ECG trace shares the lower band with panel counters; drop it when a feeder
        // sends counters or a death tally that need the room (asked 2026-10-13). The beat
        // still ticks via `scroll` above, so the heart swell and alarm flash keep going.
        if extra.deaths.is_none() && extra.counts.iter().all(Option::is_none) && extra.foe.is_none()
        {
            self.beat.trace(bm);
        }
        let [hx, hy] = self.tuning.heart;
        let (sprite, w, h, dx, dy) = self.sprite(pct);
        // The lock-on override drops the heart sprite for the room; the player keeps the HP
        // readout, the bottom bar, and the backlight.
        if extra.foe.is_none() {
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
        }
        self.extras_by_the_sprite(bm, hx, hy, h, extra);
        let [rx, ry] = self.tuning.readout;
        readout(bm, &pct.min(999).to_string(), rx, ry);
        if let Some(foe) = extra.foe {
            self.foe_band(bm, foe);
        } else {
            self.extras_on_the_panel(bm, rx, ry, extra);
        }
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            (BAR_W * shield.min(200) as i32 + 50) / 100,
            helmet,
            &self.tuning.notches(),
            extra.cap.map(|c| (BAR_W * c.min(100) as i32 + 50) / 100),
        );
        // In the alarm band every beat flashes for a tenth of a second: the panel, or
        // the trace alone.
        if self.beat.just_beat(TICKS_PER_100MS) && self.tuning.is_alarm(pct) {
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
        // A game's own level timer (ULTRAKILL) sits by the readout; a feeder that also
        // sends a death tally gets its clock stacked under that tally below instead, so
        // skip this placement when deaths are present.
        if extra.deaths.is_none() {
            if let Some(t) = extra.time {
                let s = clock(t);
                let w = s.len() as i32 * 6 - 1;
                bm.halo(rx - w, ry + 16, w, 7);
                text(bm, rx - w, ry + 16, &s, 1);
            }
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
        // The lower band: generic counters from the left, the death tally at the right with
        // its clock (SoH's time of day) stacked beneath it. The ECG trace is suppressed in
        // `alive` while these are present, so the row is clear. A counter with a max gets a
        // thin gauge beneath it.
        let mut cx = 2;
        for c in extra.counts.iter().flatten() {
            let name = c.label.as_str().replace('_', " ");
            let label = if c.max > 0 {
                format!("{name} {}/{}", c.value, c.max)
            } else {
                format!("{name} {}", c.value)
            };
            let w = label.len() as i32 * 6 - 1;
            if cx + w > W as i32 {
                break;
            }
            bm.halo(cx, 18, w, if c.max > 0 { 11 } else { 7 });
            text(bm, cx, 18, &label, 1);
            if c.max > 0 {
                let pct = ((u64::from(c.value) * 100) / u64::from(c.max.max(1))).min(100) as u32;
                gauge(bm, cx, 26, w, 3, pct);
            }
            cx += w + 6;
        }
        if let Some(d) = extra.deaths {
            // Death tally at the right of the band; its clock stacks directly beneath it,
            // shown as H:MM so the fast in-game seconds do not jitter the readout.
            let label = format!("DEATHS {d}");
            let w = label.len() as i32 * 6 - 1;
            let dx = (W as i32 - 2 - w).max(cx);
            bm.halo(dx, 18, w, 7);
            text(bm, dx, 18, &label, 1);
            if let Some(t) = extra.time {
                let hm = format!("{:02}:{:02}", t / 3600, (t % 3600) / 60);
                let tw = hm.len() as i32 * 6 - 1;
                let tx = W as i32 - 2 - tw;
                bm.halo(tx, 26, tw, 7);
                text(bm, tx, 26, &hm, 1);
            }
        }
        // Last, so its frame is not cut by the band's halos (the picked mock drew it last).
        if let Some(deck) = extra.deck {
            self.deck_strip(bm, deck);
        }
    }

    /// The feeder's icon strip at the top centre: up to four 16x16 icons at native size,
    /// 18 px apart from x 44, between the heart and the readout; the `next` icons from
    /// `at` share one frame. The strip slides (`Deck::window`) to keep the frame in view.
    fn deck_strip(&self, bm: &mut Bitmap, deck: Deck) {
        let window = deck.window();
        let shown = (deck.len as usize - window).min(DECK_SHOWN);
        bm.halo(DECK_X, DECK_Y, shown as i32 * DECK_STEP - 2, 16);
        for i in 0..shown {
            let rows = deck.rows(window + i);
            let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
            bm.sprite(DECK_X + DECK_STEP * i as i32, DECK_Y, &rows);
        }
        let first = (deck.at as usize).saturating_sub(window);
        let last = (deck.at as usize + deck.next as usize)
            .saturating_sub(window)
            .min(shown);
        if deck.next > 0 && first < last {
            let (x0, y0) = (DECK_X + DECK_STEP * first as i32 - 2, DECK_Y - 1);
            let (x1, y1) = (DECK_X + DECK_STEP * last as i32 - 3, DECK_Y + 16);
            bm.line(x0, y0, x1, y0);
            bm.line(x0, y1, x1, y1);
            bm.line(x0, y0, x0, y1);
            bm.line(x1, y0, x1, y1);
        }
    }

    /// The lock-on override: the targeted enemy's name on top, then a wide HP bar with the
    /// current/max numbers inverted inside it (legible over fill and empty alike). Reusable
    /// by any game whose feeder sends `foe`; the player keeps the HP readout, the bottom bar
    /// and the backlight, so their own danger still reads by colour and the lower gauge.
    fn foe_band(&self, bm: &mut Bitmap, foe: Foe) {
        let name = foe.label.as_str().replace('_', " ");
        bm.halo(2, 1, name.len() as i32 * 6 - 1, 7);
        text(bm, 2, 1, &name, 1);
        // The wide enemy HP bar.
        let (bx, by, bw, bh) = (2, 10, W as i32 - 4, 16);
        let fp = if foe.max > 0 {
            ((u64::from(foe.current) * 100) / u64::from(foe.max.max(1))).min(100) as u32
        } else {
            0
        };
        gauge(bm, bx, by, bw, bh, fp);
        // The numbers, centred and inverted inside the bar.
        let num = if foe.max > 0 {
            format!("{}/{}", foe.current, foe.max)
        } else {
            foe.current.to_string()
        };
        let tw = num.chars().count() as i32 * 6 - 1;
        text_xor(bm, bx + (bw - tw) / 2, by + (bh - 7) / 2, &num, 1);
    }

    pub(super) fn vehicle(&mut self, bm: &mut Bitmap, pct: u32, shield: u32, extra: Extra) {
        // Returning on foot starts a fresh ECG instead of retaining old beats.
        self.beat.reset();
        // One turn of the gears every 4.8 s, as when a tick was 100 ms.
        let turn = 48 * TICKS_PER_100MS;
        let phase = (self.tick % turn) as i32;
        let tenths = TICKS_PER_100MS as i32;
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
                    if (x + phase / (2 * tenths)) % 4 < 2 {
                        bm.plot(x, 11);
                        bm.plot(30 - x, 14);
                    }
                }
            }
            VehicleStyle::Gear => {
                // Whole tenths exactly as the 100 ms frames computed them (the crack
                // mask is sensitive to a last-bit difference), the ticks between
                // added as fine steps.
                let angle = (phase / tenths) as f32 * std::f32::consts::TAU / 48.0
                    + (phase % tenths) as f32 * std::f32::consts::TAU / turn as f32;
                let damaged = self.tuning.vehicle_damaged(pct);
                // Adjacent wheels counter-rotate; the middle meshes with both.
                gear(bm, 7, 6, angle, damaged);
                gear(bm, 16, 12, -angle, damaged);
                gear(bm, 25, 6, angle, damaged);
            }
            VehicleStyle::Scan => {
                gauge(bm, 3, 2, 26, 18, 0);
                let step = (phase / tenths) % 48;
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
                                          // A 1.2 s pause, then two columns every tenth of a second.
            let pause = 12 * TICKS_PER_100MS;
            let cycle = pause + (loop_width as u32).div_ceil(2) * TICKS_PER_100MS;
            let phase = self.tick.wrapping_sub(self.research_started) % cycle;
            let offset = phase.saturating_sub(pause) as i32 * 2 / TICKS_PER_100MS as i32;
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
        let tenths = TICKS_PER_100MS as i32;
        let full = t % (20 * tenths) < 3 * tenths;
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
        let x = BAR_X + tri(t * 2 / tenths, BAR_W - 6);
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

/// Mockup-only: compose the three candidate ring-page directions from the real glyph and
/// bar primitives, so a feh sheet shows the true font and pixel grid rather than a drawing.
/// Guarded by an env var like the other frame dumps; a no-op in the normal suite.
#[cfg(test)]
#[test]
fn dump_ring_mockups() {
    let Some(dir) = std::env::var_os("G13MAP_RING_DUMP") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    fn plot(bm: &mut Bitmap, x: i32, y: i32) {
        if (0..160).contains(&x) && (0..43).contains(&y) {
            bm.set(x as usize, y as usize, true);
        }
    }
    // A filled annulus for the ring glyph: thick when you hold rings, a thin outline at zero.
    fn ring(bm: &mut Bitmap, cx: i32, cy: i32, r_out: i32, r_in: i32) {
        for y in (cy - r_out)..=(cy + r_out) {
            for x in (cx - r_out)..=(cx + r_out) {
                let d2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                if d2 <= r_out * r_out && d2 >= r_in * r_in {
                    plot(bm, x, y);
                }
            }
        }
    }
    fn tw(s: &str, scale: i32) -> i32 {
        (s.chars().count() as i32 * 6 - 1) * scale
    }
    fn ctext(bm: &mut Bitmap, s: &str, scale: i32, y: i32) {
        text(bm, 80 - tw(s, scale) / 2, y, s, scale);
    }
    fn cread(bm: &mut Bitmap, s: &str, y: i32) {
        let w = s.len() as i32 * 12 - 2;
        readout(bm, s, 80 + w / 2, y);
    }
    fn dump(dir: &std::path::Path, name: &str, bm: &Bitmap) {
        let mut pbm = String::from("P1\n160 43\n");
        for y in 0..43 {
            for x in 0..160 {
                pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
            }
            pbm.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
    }

    for (tag, rings) in [("has", "24"), ("zero", "0")] {
        let has = tag == "has";

        // A: number only; has/zero lives entirely in the backlight colour.
        let mut a = Bitmap::blank();
        ctext(&mut a, "RINGS", 1, 6);
        cread(&mut a, rings, 20);
        dump(&dir, &format!("a-{tag}"), &a);

        // B: a ring glyph that also carries the state on the glass (solid = has, thin = zero).
        let mut b = Bitmap::blank();
        ring(&mut b, 45, 21, 11, if has { 5 } else { 9 });
        readout(&mut b, rings, 118, 14);
        dump(&dir, &format!("b-{tag}"), &b);

        // C: a solid strip where the health bar used to be, lit = alive, dark at zero.
        let mut c = Bitmap::blank();
        ctext(&mut c, "RINGS", 1, 3);
        cread(&mut c, rings, 11);
        if has {
            for yy in 34..40 {
                for xx in BAR_X..BAR_X + BAR_W {
                    plot(&mut c, xx, yy);
                }
            }
        }
        dump(&dir, &format!("c-{tag}"), &c);
    }
}

/// Mockup-only: the spinning-ring direction (Sonic's edge-on ring rotation) beside the
/// count, as a sequence of real frames for an animated GIF. Same env guard as the sheet.
#[cfg(test)]
#[test]
fn dump_ring_spin() {
    let Some(dir) = std::env::var_os("G13MAP_RINGSPIN_DUMP") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    fn plot(bm: &mut Bitmap, x: i32, y: i32) {
        if (0..160).contains(&x) && (0..43).contains(&y) {
            bm.set(x as usize, y as usize, true);
        }
    }
    // A ring turning about its vertical axis: the horizontal radius shrinks with the angle
    // while the height holds, so a full circle squishes to a thin vertical sliver and back.
    fn spin_ring(bm: &mut Bitmap, cx: i32, cy: i32, r: f32, th: f32, angle: f32) {
        // Floor the horizontal radius at 1.5 so the edge-on ring is 3 px across, not a
        // hairline: it reads with the band's depth rather than flat (asked 2026-10-14).
        let rx_out = (r * angle.cos().abs()).max(1.5);
        let ry_out = r;
        let rx_in = rx_out * (r - th) / r;
        let ry_in = ry_out - th;
        let bound = r.ceil() as i32 + 1;
        for y in -bound..=bound {
            for x in -bound..=bound {
                let (xf, yf) = (x as f32, y as f32);
                // Strict `<` drops the lone pixels sitting exactly on the radius at the four
                // cardinal points, so the ring's top/bottom (and left/right at full width)
                // read as flat edges instead of single-pixel tips (asked 2026-10-14).
                let in_out = (xf * xf) / (rx_out * rx_out) + (yf * yf) / (ry_out * ry_out) < 1.0;
                // Near edge-on the ring is a solid sliver (no visible hole), like the sprite.
                let out_in = rx_out < 2.0
                    || rx_in < 0.5
                    || (xf * xf) / (rx_in * rx_in) + (yf * yf) / (ry_in * ry_in) >= 1.0;
                if in_out && out_in {
                    plot(bm, cx + x, cy + y);
                }
            }
        }
    }
    fn dump(dir: &std::path::Path, name: &str, bm: &Bitmap) {
        let mut pbm = String::from("P1\n160 43\n");
        for y in 0..43 {
            for x in 0..160 {
                pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
            }
            pbm.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
    }

    const FRAMES: i32 = 8;
    for i in 0..FRAMES {
        let angle = std::f32::consts::PI * i as f32 / FRAMES as f32; // half turn looks identical
        let mut bm = Bitmap::blank();
        spin_ring(&mut bm, 46, 21, 9.0, 4.0, angle);
        readout(&mut bm, "24", 120, 14);
        dump(&dir, &format!("spin-{i:02}"), &bm);
    }
    // Zero state: no ring to spin, just the count.
    let mut z = Bitmap::blank();
    readout(&mut z, "0", 120, 14);
    dump(&dir, "spin-zero", &z);
}

/// Mockup-only: the three candidate layouts for the kitty/Claude terminal page, each in the
/// normal "generating" state, composed from the real glyphs so the feh sheet shows the true
/// font and grid. The fork the sheet settles: which fact gets the big numerals. Attention
/// states (inversion + backlight colour) and the pulse are motion — they go in a GIF, not
/// here. Guarded by G13MAP_TERMINAL_DUMP like the other dumps; a no-op in the normal suite.
#[cfg(test)]
#[test]
fn dump_terminal_mockups() {
    let Some(dir) = std::env::var_os("G13MAP_TERMINAL_DUMP") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    fn dump(dir: &std::path::Path, name: &str, bm: &Bitmap) {
        let mut pbm = String::from("P1\n160 43\n");
        for y in 0..43 {
            for x in 0..160 {
                pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
            }
            pbm.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
    }

    // Sample turn state the three share: mid-turn, a prior turn to show, two agents out.
    let now = "00:01:23:45"; // HH:MM:SS:CC, the 50 fps hundredths
    let last = "00:02:58:10";

    // A: the running clock is the hero. Activity on the top row, prior turn and agent count
    // tucked along the bottom. The bet: "how long has this turn run" is the glance.
    let mut a = Bitmap::blank();
    a.sprite(3, 0, HEART);
    text(&mut a, 13, 0, "EDITING", 1);
    readout(&mut a, now, 145, 15); // 11 chars * 12 - 2 = 130 wide, centred on 80
    text(&mut a, 3, 36, "LAST 00:02:58", 1);
    text(&mut a, 110, 36, "2 AGENTS", 1); // 8 chars * 6 - 1 = 47 wide, right edge 157
    dump(&dir, "a", &a);

    // B: what I'm doing is the hero. Big heart + activity word; clock, prior turn and agents
    // small below. The bet: the *kind* of work matters more at a glance than the count.
    let mut b = Bitmap::blank();
    b.sprite(4, 3, HEART_BIG);
    text(&mut b, 17, 3, "EDITING", 2); // 7 chars * 12 - 2 = 82 wide
    text(&mut b, 4, 24, now, 1); // 11 chars * 6 - 1 = 65 wide
    text(&mut b, 110, 24, "2 AGENTS", 1);
    text(&mut b, 4, 34, "LAST 00:02:58", 1);
    dump(&dir, "b", &b);

    // C: two columns, nothing hero-sized. Activity (and agents) left, a NOW/LAST clock stack
    // right. The bet: you want all four facts up at once and read the one you need.
    let mut c = Bitmap::blank();
    c.line(80, 0, 80, 42);
    c.sprite(2, 3, HEART);
    text(&mut c, 13, 3, "EDITING", 1);
    text(&mut c, 2, 32, "2 AGENTS", 1);
    text(&mut c, 86, 1, "NOW", 1);
    text(&mut c, 86, 10, now, 1);
    text(&mut c, 86, 23, "LAST", 1);
    text(&mut c, 86, 32, last, 1);
    dump(&dir, "c", &c);
}

/// Mockup-only: option B (the picked layout) in motion, as a numbered PBM sequence plus a
/// `frames.txt` manifest (`name colour delay_ms bright`). Shows what a still cannot: the
/// heart pulse, the ticking hundredths, and the four attention states. "blink" is baked in
/// as a whole-panel inversion of the pixels; colour (backlight) and the breathing brightness
/// are applied by the assembler. Guarded by G13MAP_TERMINAL_ANIM; a no-op in the suite.
#[cfg(test)]
#[test]
fn dump_terminal_anim() {
    let Some(dir) = std::env::var_os("G13MAP_TERMINAL_ANIM") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    // The B layout: big heart (contracted on the beat) + activity word, then the stopwatch,
    // agent count and prior-turn length small below.
    fn page(activity: &str, now: &str, beat: bool) -> Bitmap {
        let mut bm = Bitmap::blank();
        if beat {
            bm.sprite(5, 4, HEART); // the contraction: smaller heart, same centre
        } else {
            bm.sprite(4, 3, HEART_BIG);
        }
        text(&mut bm, 17, 3, activity, 2);
        text(&mut bm, 4, 24, now, 1);
        text(&mut bm, 110, 24, "2 AGENTS", 1);
        text(&mut bm, 4, 34, "LAST 00:02:58", 1);
        bm
    }
    fn inverted(bm: &Bitmap) -> Bitmap {
        let mut o = bm.clone();
        for y in 0..H {
            for x in 0..W {
                o.set(x, y, !bm.get(x, y));
            }
        }
        o
    }
    // Centiseconds as HH:MM:SS:CC, the stopwatch's own format.
    fn cc(total: u32) -> String {
        let (h, m, s, c) = (
            total / 360_000,
            total / 6_000 % 60,
            total / 100 % 60,
            total % 100,
        );
        format!("{h:02}:{m:02}:{s:02}:{c:02}")
    }

    let mut manifest = String::new();
    let mut n = 0u32;
    let mut emit = |bm: &Bitmap, colour: &str, delay: u32, bright: u32, manifest: &mut String| {
        let name = format!("f{n:03}");
        let mut pbm = String::from("P1\n160 43\n");
        for y in 0..43 {
            for x in 0..160 {
                pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
            }
            pbm.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
        manifest.push_str(&format!("{name} {colour} {delay} {bright}\n"));
        n += 1;
    };

    let mut t = 8_345u32; // 00:01:23:45, where the still sat
    let step = 6; // centiseconds per frame, so the hundredths visibly roll

    // GENERATING: heart beats, clock ticks, the activity face changes. Cyan, upright.
    for i in 0..16u32 {
        let activity = if i < 8 { "EDITING" } else { "WEB SEARCH" };
        let beat = i % 8 < 2; // a short contraction at the top of each beat
        emit(
            &page(activity, &cc(t), beat),
            "cyan",
            60,
            100,
            &mut manifest,
        );
        t += step;
    }
    // PERMISSION (blocked): repeating yellow blink until he answers. Clock still runs.
    for i in 0..8u32 {
        let p = page("PERMISSION", &cc(t), false);
        let frame = if i % 2 == 1 { inverted(&p) } else { p };
        emit(&frame, "yellow", 150, 100, &mut manifest);
        t += step;
    }
    // READY (turn ended, Stop fired): green, blinks twice, then holds. Clock frozen.
    let done = cc(t);
    for i in 0..6u32 {
        let r = page("READY", &done, false);
        let frame = if i == 1 || i == 3 { inverted(&r) } else { r };
        emit(&frame, "green", 150, 100, &mut manifest);
    }
    // IDLE (60 s later, still away): green backlight breathes. Upright, clock frozen.
    for &bright in &[60u32, 72, 84, 96, 96, 84, 72, 60] {
        emit(
            &page("READY", &done, false),
            "green",
            110,
            bright,
            &mut manifest,
        );
    }

    std::fs::write(dir.join("frames.txt"), manifest).unwrap();
}
