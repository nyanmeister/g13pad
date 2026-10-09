// SPDX-License-Identifier: GPL-3.0-or-later
//! How the G13's glass looks on a monitor: the LCD is lit by the backlight LEDs, so a
//! frame drawn to a screen is the backlight colour for a dark pixel and a lighter tint
//! of it for a lit one. The LEDs are not a monitor's primaries (the red is weak next
//! to the green and blue: the value that is orange on the glass is nearly red on a
//! monitor), so a table in `~/.config/g13map/glass` translates LED values to what the
//! glass shows, one `R G B  R G B` line each (LED, then monitor), tuned by eye against
//! the pad (asked 2026-10-08). A colour not in the table is shown as it is. Anything
//! in the project that renders the LCD to a monitor should draw through here.
use crate::lcd::{Bitmap, H, W};
use std::fs;

pub type Rgb = [u8; 3];

/// Lit pixels sit this far from the background toward white.
pub const LIT: f32 = 0.55;

/// The built-in translation, the one the README's GIF was made with.
const DEFAULT: &[(Rgb, Rgb)] = &[([0, 255, 0], [0, 150, 0]), ([255, 48, 0], [255, 128, 0])];

#[derive(Clone, Debug, PartialEq)]
pub struct Glass {
    table: Vec<(Rgb, Rgb)>,
}

impl Default for Glass {
    fn default() -> Self {
        Glass {
            table: DEFAULT.to_vec(),
        }
    }
}

impl Glass {
    /// The user's table, else the built-in one.
    pub fn load() -> Glass {
        fs::read_to_string(crate::config_dir().join("glass"))
            .ok()
            .map(|t| Glass::parse(&t))
            .unwrap_or_default()
    }

    /// One `R G B  R G B` per line; `#` starts a comment; a bad line is skipped. An empty
    /// table (a file of comments) means no translation at all.
    pub fn parse(text: &str) -> Glass {
        let mut table = Vec::new();
        for line in text.lines() {
            let words: Vec<u8> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|w| w.parse().ok())
                .collect();
            if let [r, g, b, mr, mg, mb] = words[..] {
                table.push(([r, g, b], [mr, mg, mb]));
            }
        }
        Glass { table }
    }

    /// What the monitor shows for a backlight value.
    pub fn shown(&self, led: Rgb) -> Rgb {
        self.table
            .iter()
            .find(|(from, _)| *from == led)
            .map_or(led, |(_, to)| *to)
    }

    /// The two colours of a frame under this backlight: dark pixels, lit pixels.
    pub fn pair(&self, led: Rgb) -> (Rgb, Rgb) {
        let bg = self.shown(led);
        let lit = bg.map(|c| (c as f32 + (255.0 - c as f32) * LIT).round() as u8);
        (bg, lit)
    }

    /// The frame as the glass would show it, one RGBA pixel per LCD pixel.
    pub fn render(&self, frame: &Bitmap, led: Rgb) -> Vec<u8> {
        let (bg, lit) = self.pair(led);
        let mut out = Vec::with_capacity(W * H * 4);
        for y in 0..H {
            for x in 0..W {
                let c = if frame.get(x, y) { lit } else { bg };
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_translates_listed_colours_only() {
        let g = Glass::default();
        assert_eq!(g.shown([0, 255, 0]), [0, 150, 0]);
        assert_eq!(g.shown([0, 0, 255]), [0, 0, 255]);
        let own = Glass::parse("# mine\n0 0 255  40 60 255\nbad line\n1 2\n");
        assert_eq!(own.shown([0, 0, 255]), [40, 60, 255]);
        assert_eq!(own.shown([0, 255, 0]), [0, 255, 0]);
        assert_eq!(Glass::parse("# nothing").table, vec![]);
    }

    #[test]
    fn lit_pixels_tint_toward_white() {
        let (bg, lit) = Glass::default().pair([0, 0, 255]);
        assert_eq!(bg, [0, 0, 255]);
        assert_eq!(lit, [140, 140, 255]);
        let mut frame = Bitmap::blank();
        frame.set(0, 0, true);
        let px = Glass::default().render(&frame, [0, 0, 255]);
        assert_eq!(&px[..4], &[140, 140, 255, 255]);
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
        assert_eq!(px.len(), W * H * 4);
    }
}
