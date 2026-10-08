// SPDX-License-Identifier: GPL-3.0-or-later
//! Kept text appearance and the system font chooser.
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub font: Option<PathBuf>,
    pub size: f32,
    pub wrap: bool,
    pub align: Align,
    pub invert: bool,
    pub speed: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            font: None,
            size: 36.0,
            wrap: false,
            align: Align::Center,
            invert: false,
            speed: 40,
        }
    }
}

impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite() || !(6.0..=72.0).contains(&self.size) || self.speed > 120 {
            return Err("font size must be 6–72 points; scroll speed 0–120 pixels/s".into());
        }
        Ok(())
    }
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "size {}\nwrap {}\nalign {}\ninvert {}\nspeed {}\n",
            self.size,
            self.wrap as u8,
            match self.align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
            },
            self.invert as u8,
            self.speed
        );
        if let Some(font) = &self.font {
            s.push_str(&format!("font {}\n", font.display()));
        }
        s
    }
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once(' ') else {
                continue;
            };
            match key {
                "font" if !value.trim().is_empty() => o.font = Some(value.trim().into()),
                "size" => {
                    if let Ok(v) = value.parse::<f32>() {
                        if v.is_finite() {
                            o.size = v.clamp(6.0, 72.0)
                        }
                    }
                }
                "wrap" => o.wrap = value == "1",
                "invert" => o.invert = value == "1",
                "speed" => {
                    if let Ok(v) = value.parse::<u16>() {
                        o.speed = v.min(120)
                    }
                }
                "align" => {
                    o.align = match value {
                        "left" => Align::Left,
                        "right" => Align::Right,
                        _ => Align::Center,
                    }
                }
                _ => {}
            }
        }
        o
    }
    pub fn load(name: &str) -> Self {
        fs::read_to_string(crate::lcd::dir().join(format!("{name}.textopts")))
            .map(|s| Self::parse(&s))
            .unwrap_or_default()
    }
}

pub fn fonts() -> Vec<(PathBuf, String)> {
    let Ok(output) = Command::new("fc-list")
        .args(["--format", "%{file}\t%{family}\t%{style}\t%{index}\n"])
        .output()
    else {
        return vec![];
    };
    if !output.status.success() {
        return vec![];
    }
    let mut found = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split('\t');
        let (Some(path), Some(family), Some(style)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // The renderer uses face zero, including a collection's first face. Do not
        // offer other faces or variable instances under a misleading family label.
        if !path.is_empty() && parts.next() == Some("0") {
            found
                .entry(PathBuf::from(path))
                .or_insert_with(|| format!("{family} — {style}"));
        }
    }
    let mut found: Vec<_> = found.into_iter().collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_round_trip_and_reject_nonfinite_sizes() {
        let o = Options {
            font: Some("/tmp/font with spaces.ttf".into()),
            size: 18.5,
            wrap: true,
            align: Align::Right,
            invert: true,
            speed: 72,
        };
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("size NaN\nsize inf\nspeed 999\n").size, 36.0);
        assert_eq!(Options::parse("size 1\nspeed 999\n").size, 6.0);
        assert_eq!(Options::parse("speed 999\n").speed, 120);
        assert!(Options {
            size: f32::NAN,
            ..o
        }
        .validate()
        .is_err());
    }
}
