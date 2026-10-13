// SPDX-License-Identifier: GPL-3.0-or-later
//! The kit: parts a screen composes, none of which knows a game. The heartbeat and the
//! dark hold carry their own state; the pie is a pure drawing. Asked 2026-10-13: what a
//! game integration shows is its own business (what the game does not show, ideally);
//! what it shows it with should be shared.
use super::render::{BASE, SCROLL};
use super::*;

/// The scrolling ECG trace and its beat clock: a ring of column lifts, scrolled left
/// `SCROLL` columns a tick with new columns from the beat clock on the right. A pure
/// function of the ticks it was given, so tests can draw it without a clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Heartbeat {
    pub(super) lift: [i32; W],
    /// Columns since the current beat began.
    pub(super) u: i32,
    /// Ticks since the last beat began; `None` before the first.
    pub(super) beat_age: Option<u32>,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Heartbeat {
            lift: [0; W],
            u: 0,
            beat_age: None,
        }
    }
}

impl Heartbeat {
    /// A flat trace and no beat remembered: a fresh ECG for the next pulse.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// One tick: scroll, and add `SCROLL` new columns from the beat clock. `period` is
    /// the columns from one beat to the next; a `pct` of zero flatlines the trace.
    pub(crate) fn tick(&mut self, pct: u32, period: i32) {
        let beating = pct > 0;
        let mut beat = false;
        self.lift.rotate_left(SCROLL as usize);
        for x in W - SCROLL as usize..W {
            self.lift[x] = if beating { ecg(self.u) } else { 0 };
            // The heart pumps as the R peak enters, not as the quiet P wave does.
            beat |= beating && self.u == ECG_R;
            // A flat trace holds the clock at the start, so the next pulse begins whole.
            self.u = if beating && self.u + 1 < period {
                self.u + 1
            } else {
                0
            };
        }
        self.beat_age = match (beat, self.beat_age) {
            (true, _) => Some(0),
            (false, Some(a)) => Some(a + 1),
            (false, None) => None,
        };
    }

    /// The trace across the glass, with its baseline at `BASE`.
    pub(crate) fn trace(&self, bm: &mut Bitmap) {
        for x in 0..W as i32 - 1 {
            let (a, b) = (self.lift[x as usize], self.lift[x as usize + 1]);
            bm.line(x, BASE - a, x + 1, BASE - b);
        }
    }

    /// True for `ticks` from the pump, the beat's own moment: a flash, a pip.
    pub(crate) fn just_beat(&self, ticks: u32) -> bool {
        self.beat_age.is_some_and(|a| a < ticks)
    }

    /// True for `swell_ticks` after a beat: the sprite's moment to swell or blink.
    pub(crate) fn swelling(&self, swell_ticks: u32) -> bool {
        self.beat_age.is_some_and(|a| a < swell_ticks)
    }
}

/// The dark seconds after a drop to zero: a death reads as a death even if the feed
/// moves on at once (asked 2026-10-07).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hold {
    /// Ticks at zero.
    pub(super) dead: u32,
}

impl Hold {
    /// One tick: counting while `zero` or while already inside the hold.
    pub(crate) fn tick(&mut self, zero: bool, hold_ticks: u32) {
        self.dead = if zero || self.holding(hold_ticks) {
            self.dead + 1
        } else {
            0
        };
    }

    /// Inside the hold.
    pub(crate) fn holding(&self, hold_ticks: u32) -> bool {
        (1..=hold_ticks).contains(&self.dead)
    }
}

/// Where a pie sits: its centre, the filled disc's radius, and the segmented ring
/// around it from `ring_in` to `ring_out`, cut into `segments`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pie {
    pub(crate) cx: i32,
    pub(crate) cy: i32,
    pub(crate) disc: i32,
    pub(crate) ring_in: i32,
    pub(crate) ring_out: i32,
    pub(crate) segments: u32,
}

impl Pie {
    /// A clockwise sector from twelve o'clock filling `fill` (0 to 1) of the disc,
    /// inside the segmented ring.
    pub(crate) fn draw(&self, bm: &mut Bitmap, fill: f32) {
        use std::f32::consts::TAU;
        let fill = fill.clamp(0.0, 1.0);
        let (disc, ring_in, ring_out) = (self.disc, self.ring_in, self.ring_out);
        for y in -ring_out..=ring_out {
            for x in -ring_out..=ring_out {
                let r2 = x * x + y * y;
                let angle = (x as f32).atan2(-y as f32).rem_euclid(TAU);
                if (r2 <= disc * disc && fill > 0.0 && angle <= fill * TAU)
                    || ((ring_in * ring_in..=ring_out * ring_out).contains(&r2)
                        && angle.rem_euclid(TAU / self.segments as f32) > 0.10)
                {
                    bm.plot(self.cx + x, self.cy + y);
                }
            }
        }
    }
}
