// SPDX-License-Identifier: GPL-3.0-or-later
//! How the G13's glass looks on a monitor: the LCD is lit by the backlight LEDs, so a
//! frame drawn to a screen is the backlight colour for a dark pixel and the same colour
//! brighter for a lit one (a clear pixel passes more of the same light). The LEDs are
//! not a monitor's primaries (the red is weak next to the green and blue: the value that
//! is orange on the glass is nearly red on a monitor), so `~/.config/g13map/glass`
//! translates LED values to what the glass shows.
//!
//! The file holds colours matched by eye against the pad, one `R G B  R G B` line each
//! (LED, then monitor), a `glow` line (how many times the background's light a lit pixel
//! passes; it drifts toward white only where a channel has no headroom left, as the eye
//! sees an over-bright colour) and a `fit` line: the linear-light monitor colour of each LED alone, solved by
//! least squares from the matched pairs (asked 2026-10-08, `g13map glass`). A listed
//! colour is shown as matched; any other goes through the fit (LED light adds linearly,
//! so three independent matches fix the whole gamut); without a fit it is shown as it is.
//! Anything in the project that renders the LCD to a monitor should draw through here,
//! and the OBS source plugin (obs/g13pad-obs.c) reads the same file the same way.
use crate::lcd::{Bitmap, H, W};
use std::{fs, path::PathBuf, time::SystemTime};

pub type Rgb = [u8; 3];

/// Linear-light monitor colour per unit of each LED: row `c` gives the monitor's
/// channel `c` as a weight on the red, green and blue LEDs.
pub type Fit = [[f32; 3]; 3];

/// Lit pixels pass this many times the background's light, unless the file says
/// (asked 2026-10-08: "the pixels look like a brighter version of the existing colour").
pub const GLOW: f32 = 2.0;

/// The built-in translation, the one the README's GIF was made with.
const DEFAULT: &[(Rgb, Rgb)] = &[([0, 255, 0], [0, 150, 0]), ([255, 48, 0], [255, 128, 0])];

#[derive(Clone, Debug, PartialEq)]
pub struct Glass {
    /// Matched by eye: LED value, what the monitor shows for it.
    pub table: Vec<(Rgb, Rgb)>,
    pub glow: f32,
    pub fit: Option<Fit>,
}

impl Default for Glass {
    fn default() -> Self {
        Glass {
            table: DEFAULT.to_vec(),
            glow: GLOW,
            fit: None,
        }
    }
}

/// sRGB byte to linear light.
pub fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light to an sRGB byte, clipped to the monitor's range.
pub fn to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

impl Glass {
    pub fn path() -> PathBuf {
        crate::config_dir().join("glass")
    }

    /// When the file last changed, if it exists.
    pub fn stamp() -> Option<SystemTime> {
        fs::metadata(Self::path()).and_then(|m| m.modified()).ok()
    }

    /// The user's table, else the built-in one.
    pub fn load() -> Glass {
        fs::read_to_string(Self::path())
            .ok()
            .map(|t| Glass::parse(&t))
            .unwrap_or_default()
    }

    /// `R G B  R G B` per line (LED, then monitor); `lit F`; `fit` and nine numbers; `#`
    /// starts a comment; a bad line is skipped. An empty table (a file of comments) means
    /// no translation at all.
    pub fn parse(text: &str) -> Glass {
        let mut glass = Glass {
            table: Vec::new(),
            glow: GLOW,
            fit: None,
        };
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut words = line.split_whitespace();
            match words.next() {
                Some("glow") => {
                    if let Some(v) = words.next().and_then(|w| w.parse::<f32>().ok()) {
                        if (1.0..=8.0).contains(&v) {
                            glass.glow = v;
                        }
                    }
                }
                Some("fit") => {
                    let v: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                    if let [a, b, c, d, e, f, g, h, i] = v[..] {
                        if v.iter().all(|x| x.is_finite()) {
                            glass.fit = Some([[a, b, c], [d, e, f], [g, h, i]]);
                        }
                    }
                }
                _ => {
                    let v: Vec<u8> = line
                        .split_whitespace()
                        .filter_map(|w| w.parse().ok())
                        .collect();
                    if let [r, g, b, mr, mg, mb] = v[..] {
                        glass.table.push(([r, g, b], [mr, mg, mb]));
                    }
                }
            }
        }
        glass
    }

    /// The file as `parse` reads it back.
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# Backlight colours as the glass shows them, matched by eye (g13map glass).\n\
             # LED R G B   monitor R G B\n",
        );
        for (led, shown) in &self.table {
            out.push_str(&format!(
                "{} {} {}   {} {} {}\n",
                led[0], led[1], led[2], shown[0], shown[1], shown[2]
            ));
        }
        out.push_str(&format!(
            "# A lit pixel passes this many times the background's light (1 to 8).\n\
             glow {:.2}\n",
            self.glow
        ));
        if let Some(fit) = &self.fit {
            out.push_str(
                "# The linear-light monitor colour of each LED alone, solved from the pairs above;\n\
                 # colours not listed go through it. `g13map glass fit` rewrites it after a hand edit.\n",
            );
            out.push_str("fit");
            for row in fit {
                for v in row {
                    out.push_str(&format!(" {v:.4}"));
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&path, self.to_text()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The matched colour for this LED value, if it is in the table.
    pub fn matched(&self, led: Rgb) -> Option<Rgb> {
        self.table
            .iter()
            .find(|(from, _)| *from == led)
            .map(|(_, to)| *to)
    }

    /// What the fit says the monitor shows for an LED value: LED light adds linearly,
    /// the result is encoded for the monitor.
    pub fn predicted(&self, led: Rgb) -> Option<Rgb> {
        let fit = self.fit?;
        let l = led.map(|v| v as f32 / 255.0);
        Some(fit.map(|row| to_srgb(row[0] * l[0] + row[1] * l[1] + row[2] * l[2])))
    }

    /// What the monitor shows for a backlight value: matched, else predicted, else as it is.
    pub fn shown(&self, led: Rgb) -> Rgb {
        self.matched(led)
            .or_else(|| self.predicted(led))
            .unwrap_or(led)
    }

    /// Solves the fit from the table by least squares in linear light and keeps it. None
    /// (and no fit) when the pairs do not span the three LEDs: fewer than three, or all
    /// on one line through black, such as blue at three brightnesses.
    pub fn refit(&mut self) -> Option<Fit> {
        let mut a = [[0f64; 3]; 3];
        let mut b = [[0f64; 3]; 3];
        for (led, shown) in &self.table {
            let l = led.map(|v| v as f64 / 255.0);
            let t = shown.map(|v| to_linear(v) as f64);
            for i in 0..3 {
                for j in 0..3 {
                    a[i][j] += l[i] * l[j];
                    b[i][j] += l[i] * t[j];
                }
            }
        }
        let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        if det.abs() < 1e-9 {
            self.fit = None;
            return None;
        }
        let inv = |r: usize, c: usize| {
            // Cofactor of a[c][r], over the determinant: the inverse's entry (r, c).
            let (r1, r2) = ((c + 1) % 3, (c + 2) % 3);
            let (c1, c2) = ((r + 1) % 3, (r + 2) % 3);
            (a[r1][c1] * a[r2][c2] - a[r1][c2] * a[r2][c1]) / det
        };
        let mut fit = [[0f32; 3]; 3];
        for (ch, row) in fit.iter_mut().enumerate() {
            for (k, weight) in row.iter_mut().enumerate() {
                *weight = (0..3).map(|j| inv(k, j) * b[j][ch]).sum::<f64>() as f32;
            }
        }
        self.fit = Some(fit);
        self.fit
    }

    /// The largest difference, in monitor units, between a matched colour and what the
    /// fit would have shown for it, with the LED value it belongs to: how well a linear
    /// mix of the LEDs explains the eye's matches.
    pub fn worst_miss(&self) -> Option<(u8, Rgb)> {
        self.table
            .iter()
            .filter_map(|(led, shown)| {
                let p = self.predicted(*led)?;
                let miss = (0..3).map(|i| p[i].abs_diff(shown[i])).max().unwrap_or(0);
                Some((miss, *led))
            })
            .max_by_key(|(miss, _)| *miss)
    }

    /// The two colours of a frame under this backlight: dark pixels, lit pixels.
    pub fn pair(&self, led: Rgb) -> (Rgb, Rgb) {
        let bg = self.shown(led);
        (bg, glow(bg, self.glow))
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

/// The background's light `times` over, in linear light: the same colour brighter while
/// every channel fits the monitor; past that the overflow goes to white, so a colour with
/// no headroom (full blue) reads as a pale version rather than clipping to itself. The
/// OBS source plugin applies the same formula.
pub fn glow(bg: Rgb, times: f32) -> Rgb {
    let l = bg.map(|c| to_linear(c) * times);
    let m = l[0].max(l[1]).max(l[2]);
    if m <= 1.0 {
        return l.map(to_srgb);
    }
    let white = 1.0 - 1.0 / m;
    l.map(|c| to_srgb(c / m * (1.0 - white) + white))
}

/// `g13map glass fit`: solve the fit from the file's pairs and write it back.
pub fn refit_file() -> Result<String, String> {
    let path = Glass::path();
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut glass = Glass::parse(&text);
    let n = glass.table.len();
    match glass.refit() {
        Some(_) => {
            glass.save()?;
            let miss = glass
                .worst_miss()
                .map(|(m, led)| {
                    format!(
                        "; the fit misses a match by at most {m} (LED {} {} {})",
                        led[0], led[1], led[2]
                    )
                })
                .unwrap_or_default();
            Ok(format!(
                "fit from {n} pairs written to {}{miss}",
                path.display()
            ))
        }
        None => Err(format!(
            "{n} pair(s) in {} do not span the three LEDs; match red, green and blue alone first",
            path.display()
        )),
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
    fn lit_pixels_are_the_background_brighter() {
        // Headroom: the same colour, twice the light, no drift toward white.
        assert_eq!(glow([0, 150, 0], 2.0), [0, 205, 0]);
        assert_eq!(glow([0, 150, 0], 1.0), [0, 150, 0]);
        assert_eq!(glow([0, 0, 0], 3.0), [0, 0, 0]);
        // No headroom: full blue cannot get bluer, so half the overflow is white.
        let (bg, lit) = Glass::default().pair([0, 0, 255]);
        assert_eq!(bg, [0, 0, 255]);
        assert_eq!(lit, [188, 188, 255]);
        // A mixed colour keeps its hue order while brightening.
        let [r, g, b] = glow([91, 86, 131], 2.0);
        assert!(b > r && r > g && b > 131, "{r} {g} {b}");
        let mut frame = Bitmap::blank();
        frame.set(0, 0, true);
        let px = Glass::default().render(&frame, [0, 0, 255]);
        assert_eq!(&px[..4], &[188, 188, 255, 255]);
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
        assert_eq!(px.len(), W * H * 4);
        let dim = Glass::parse("glow 1.5\n");
        assert_eq!(dim.pair([0, 150, 0]).1, glow([0, 150, 0], 1.5));
    }

    #[test]
    fn glow_and_fit_lines_parse_and_bad_ones_are_ignored() {
        let g = Glass::parse(
            "glow 0.5\nglow 9\nglow x\nlit 0.55\nfit 1 2 3\nfit 1 0 0 0 1 0 0 0 nan\n",
        );
        assert_eq!((g.glow, g.fit), (GLOW, None));
        let g = Glass::parse("glow 3 # bright\nfit 1 0 0  0 1 0  0 0 1\n");
        assert_eq!(g.glow, 3.0);
        assert_eq!(
            g.fit,
            Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
        );
        // An LED value is linear light; the identity fit still encodes it for the monitor.
        for v in [0u8, 1, 10, 128, 200, 255] {
            let want = to_srgb(v as f32 / 255.0);
            assert_eq!(g.predicted([v, v, v]), Some([want; 3]), "{v}");
        }
        assert_eq!(g.predicted([128, 128, 128]), Some([188; 3]));
    }

    #[test]
    fn srgb_round_trips() {
        for v in 0..=255u8 {
            assert_eq!(to_srgb(to_linear(v)), v, "{v}");
        }
        assert_eq!(to_srgb(-1.0), 0);
        assert_eq!(to_srgb(2.0), 255);
    }

    /// A made-up pad whose LEDs are not the monitor's primaries: pairs generated from it
    /// are enough to recover it, and colours it never listed come out as it would show them.
    #[test]
    fn fit_recovers_the_pad_from_matches() {
        // Rows sum to at most one: a monitor can show every mix without clipping.
        let truth: Fit = [[0.9, 0.05, 0.0], [0.1, 0.6, 0.1], [0.0, 0.1, 0.9]];
        let oracle = Glass {
            table: vec![],
            glow: GLOW,
            fit: Some(truth),
        };
        let leds: [Rgb; 7] = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 255],
            [19, 0, 127],
            [99, 127, 0],
            [255, 48, 0],
        ];
        let mut g = Glass {
            table: leds
                .iter()
                .map(|&led| (led, oracle.predicted(led).unwrap()))
                .collect(),
            glow: GLOW,
            fit: None,
        };
        let fit = g.refit().expect("seven pairs span the LEDs");
        for (row, want) in fit.iter().zip(truth.iter()) {
            for (a, b) in row.iter().zip(want.iter()) {
                assert!((a - b).abs() < 0.02, "{fit:?} vs {truth:?}");
            }
        }
        assert!(g.worst_miss().unwrap().0 <= 2, "{:?}", g.worst_miss());
        for led in [[0, 113, 127], [8, 144, 0], [128, 128, 128], [255, 215, 0]] {
            let (p, want) = (g.predicted(led).unwrap(), oracle.predicted(led).unwrap());
            for i in 0..3 {
                assert!(p[i].abs_diff(want[i]) <= 2, "{led:?}: {p:?} vs {want:?}");
            }
        }
        // A listed colour wins over the fit even when the eye disagreed with it.
        g.table.push(([0, 113, 127], [1, 2, 3]));
        assert_eq!(g.shown([0, 113, 127]), [1, 2, 3]);
        assert_eq!(g.shown([0, 0, 0]), [0, 0, 0]);
    }

    #[test]
    fn fit_needs_three_independent_leds() {
        let mut two = Glass::parse("255 0 0  255 0 0\n0 255 0  0 255 0\n");
        assert_eq!(two.refit(), None);
        let mut blues = Glass::parse("0 0 255  0 0 255\n0 0 128  0 0 188\n0 0 64  0 0 137\n");
        assert_eq!(blues.refit(), None);
        assert_eq!(blues.fit, None);
        let mut three = Glass::parse("255 0 0  255 0 0\n0 255 0  0 255 0\n0 0 255  0 0 255\n");
        let fit = three.refit().unwrap();
        for (i, row) in fit.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((v - want).abs() < 1e-4, "{fit:?}");
            }
        }
    }

    #[test]
    fn text_round_trips() {
        let mut g =
            Glass::parse("255 0 0  250 40 10\n0 255 0  30 160 20\n0 0 255  20 40 255\nglow 2.5\n");
        g.refit().unwrap();
        let back = Glass::parse(&g.to_text());
        assert_eq!(back.table, g.table);
        assert_eq!(back.glow, g.glow);
        let (a, b) = (back.fit.unwrap(), g.fit.unwrap());
        for i in 0..3 {
            for j in 0..3 {
                assert!((a[i][j] - b[i][j]).abs() < 1e-3);
            }
        }
        assert_eq!(back.to_text(), g.to_text());
    }
}
