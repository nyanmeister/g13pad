// SPDX-License-Identifier: GPL-3.0-or-later
//! Pixel drawing on an LCD bitmap: clipped plots, lines, discs, sprites, and the small
//! helpers the scenes (`art`) and the health meter (`meter`) share.
#![cfg_attr(not(feature = "art"), allow(dead_code))]
use crate::lcd::{Bitmap, H, W};
use std::f32::consts::TAU;

// ---- drawing: clipped plots, lines, discs and sprites on a Bitmap ----

/// Sprites are rows of text: `#` lights a pixel, `o` darkens one, anything else leaves it.
pub(crate) trait Draw {
    fn put(&mut self, x: i32, y: i32, on: bool);
    fn plot(&mut self, x: i32, y: i32) {
        self.put(x, y, true);
    }
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32);
    fn disc(&mut self, cx: i32, cy: i32, r: i32);
    /// Darkens the rectangle and a one-pixel border around it: a halo that separates a
    /// sprite from what is behind it.
    fn halo(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn sprite(&mut self, x: i32, y: i32, rows: &[&str]);
    /// The sprite mirrored left to right.
    fn sprite_flipped(&mut self, x: i32, y: i32, rows: &[&str]);
}

impl Draw for Bitmap {
    fn put(&mut self, x: i32, y: i32, on: bool) {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
            self.set(x as usize, y as usize, on);
        }
    }
    fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let mut err = dx + dy;
        loop {
            self.plot(x0, y0);
            if x0 == x1 && y0 == y1 {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }
    fn disc(&mut self, cx: i32, cy: i32, r: i32) {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y <= r * r + r / 2 {
                    self.plot(cx + x, cy + y);
                }
            }
        }
    }
    fn halo(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for yy in y - 1..=y + h {
            for xx in x - 1..=x + w {
                self.put(xx, yy, false);
            }
        }
    }
    fn sprite(&mut self, x: i32, y: i32, rows: &[&str]) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                match c {
                    b'#' => self.put(x + dx as i32, y + dy as i32, true),
                    b'o' => self.put(x + dx as i32, y + dy as i32, false),
                    _ => {}
                }
            }
        }
    }
    fn sprite_flipped(&mut self, x: i32, y: i32, rows: &[&str]) {
        let w = rows.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                match c {
                    b'#' => self.put(x + w - 1 - dx as i32, y + dy as i32, true),
                    b'o' => self.put(x + w - 1 - dx as i32, y + dy as i32, false),
                    _ => {}
                }
            }
        }
    }
}

/// A fixed scatter: the same three numbers always give the same result.
pub(crate) fn hash(a: u32, b: u32, c: u32) -> u32 {
    let mut h =
        a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

/// `amp * sin` of a phase in turns, rounded to pixels.
pub(crate) fn wave(turns: f32, amp: f32) -> i32 {
    (amp * (turns * TAU).sin()).round() as i32
}

/// A triangle wave: up from 0 to `span`, back down, period `2 * span`.
pub(crate) fn tri(u: i32, span: i32) -> i32 {
    let m = u.rem_euclid(2 * span);
    if m <= span {
        m
    } else {
        2 * span - m
    }
}

/// `n` frames and the closing repeat of the first, each drawn by `f` on a blank panel.
pub(crate) fn frames(n: usize, f: impl Fn(i32, &mut Bitmap)) -> Vec<Bitmap> {
    (0..=n)
        .map(|t| {
            let mut bm = Bitmap::blank();
            f((t % n) as i32, &mut bm);
            bm
        })
        .collect()
}

/// Columns one ECG complex takes, P wave to the end of the T wave: the shortest beat
/// the meter can show, 0.8 s at its 50 columns a second (asked 2026-10-08: beats from
/// 2.0 s calm to 0.8 s racing; the complex was 60 columns, which floored racing at 1.2 s).
pub(crate) const ECG_COLUMNS: i32 = 40;
/// The column of the spike's top in `ecg`.
pub(crate) const ECG_R: i32 = 21;

/// One ECG complex, column by column: the lift above the baseline at `u` columns after
/// the beat starts (a P wave, the QRS spike, a T wave over `ECG_COLUMNS`, then flat).
pub(crate) fn ecg(u: i32) -> i32 {
    match u {
        4..=11 => [0, 1, 1, 2, 2, 1, 1, 0][(u - 4) as usize],
        18 => -1,
        19 => -2,
        20 => 6,
        ECG_R => 14,
        22 => 10,
        23 => 2,
        24 => -5,
        25 => -3,
        26 => -1,
        30..=39 => [0, 1, 2, 2, 3, 3, 2, 2, 1, 0][(u - 30) as usize],
        _ => 0,
    }
}
