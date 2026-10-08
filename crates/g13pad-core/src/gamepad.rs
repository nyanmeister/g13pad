// SPDX-License-Identifier: GPL-3.0-or-later
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stick {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Click {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Back,
    Start,
    Guide,
    #[default]
    L3,
    R3,
    None,
}

impl Click {
    pub const ALL: [Self; 12] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::Lb,
        Self::Rb,
        Self::Back,
        Self::Start,
        Self::Guide,
        Self::L3,
        Self::R3,
        Self::None,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Lb => "LB",
            Self::Rb => "RB",
            Self::Back => "Back",
            Self::Start => "Start",
            Self::Guide => "Guide",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::None => "disabled",
        }
    }
    pub fn token(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::X => "x",
            Self::Y => "y",
            Self::Lb => "lb",
            Self::Rb => "rb",
            Self::Back => "back",
            Self::Start => "start",
            Self::Guide => "guide",
            Self::L3 => "l3",
            Self::R3 => "r3",
            Self::None => "none",
        }
    }
    pub fn source_button(self) -> &'static str {
        match self {
            Self::L3 | Self::None => "TL",
            Self::R3 => "TR",
            _ => self.token(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub stick: Stick,
    pub click: Click,
    pub swap: bool,
    pub invert_x: bool,
    pub invert_y: bool,
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            stick: Stick::Left,
            click: Click::L3,
            swap: false,
            invert_x: false,
            invert_y: true,
        }
    }
}

impl Mapping {
    pub fn compact(self) -> String {
        format!(
            "{} {} {} {} {}",
            if self.stick == Stick::Left {
                "left"
            } else {
                "right"
            },
            self.click.token(),
            self.swap as u8,
            self.invert_x as u8,
            self.invert_y as u8
        )
    }
    pub fn from_compact(text: &str) -> Result<Self, String> {
        if text.len() > 512 {
            return Err("gamepad mapping exceeds 512 bytes".into());
        }
        let fields: Vec<_> = text.split_whitespace().collect();
        let [stick, click, swap, x, y] = fields.as_slice() else {
            return Err("gamepad needs stick, click, swap, invert-X and invert-Y".into());
        };
        Self::parse(&format!(
            "stick {stick}\nclick {click}\nswap {swap}\ninvert_x {x}\ninvert_y {y}\n"
        ))
    }
    pub fn to_text(self) -> String {
        let fields = self.compact();
        let fields: Vec<_> = fields.split_whitespace().collect();
        format!(
            "stick {}\nclick {}\nswap {}\ninvert_x {}\ninvert_y {}\n",
            fields[0], fields[1], fields[2], fields[3], fields[4]
        )
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 512 {
            return Err("gamepad mapping exceeds 512 bytes".into());
        }
        let mut mapping = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        let boolean = |s: &str| match s {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err("mapping flags must be 0 or 1".to_string()),
        };
        for line in text.lines() {
            let fields: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if fields.is_empty() {
                continue;
            }
            let [key, value] = fields.as_slice() else {
                return Err("mapping settings need one value".into());
            };
            if !seen.insert(*key) {
                return Err(format!("duplicate mapping setting: {key}"));
            }
            match *key {
                "stick" => {
                    mapping.stick = match *value {
                        "left" => Stick::Left,
                        "right" => Stick::Right,
                        _ => return Err("stick must be left or right".into()),
                    }
                }
                "click" => {
                    mapping.click = Click::ALL
                        .into_iter()
                        .find(|c| c.token() == *value)
                        .ok_or("unknown gamepad click button")?
                }
                "swap" => mapping.swap = boolean(value)?,
                "invert_x" => mapping.invert_x = boolean(value)?,
                "invert_y" => mapping.invert_y = boolean(value)?,
                _ => return Err(format!("unknown mapping setting: {key}")),
            }
        }
        Ok(mapping)
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0o4000 | 0o400000 | 0o2000000)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("mapping must be a regular file".into());
        }
        file.try_lock_shared().map_err(|e| e.to_string())?;
        let mut text = String::new();
        file.take(513)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        Self::parse(&text)
    }
    /// Assign evdev axes to the selected internal stick. This avoids duplicate output
    /// mappings while retaining xboxdrv's full Xbox controller capabilities.
    pub fn axes(self) -> (&'static str, &'static str, &'static str, &'static str) {
        let (x, y, abs_x, abs_y) = match self.stick {
            Stick::Left => ("x1", "y1", "ABS_X", "ABS_Y"),
            Stick::Right => ("x2", "y2", "ABS_RX", "ABS_RY"),
        };
        if self.swap {
            (y, x, abs_y, abs_x)
        } else {
            (x, y, abs_x, abs_y)
        }
    }
    pub fn absmap(self) -> String {
        let (x, y, _, _) = self.axes();
        format!("ABS_X={x},ABS_Y={y}")
    }
    pub fn axismap(self) -> String {
        let (x, y, ax, ay) = self.axes();
        let flip = |invert| if invert { "^resp:32767:0:-32768" } else { "" };
        format!(
            "{x}{}={ax},{y}{}={ay}",
            flip(self.invert_x),
            flip(self.invert_y)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_mappings_round_trip_and_preserve_axis_calibration_identity() {
        for stick in [Stick::Left, Stick::Right] {
            for click in Click::ALL {
                for flags in 0..8 {
                    let m = Mapping {
                        stick,
                        click,
                        swap: flags & 1 != 0,
                        invert_x: flags & 2 != 0,
                        invert_y: flags & 4 != 0,
                    };
                    assert_eq!(Mapping::parse(&m.to_text()).unwrap(), m);
                    assert_eq!(Mapping::from_compact(&m.compact()).unwrap(), m);
                    let (x, y, ax, ay) = m.axes();
                    assert_ne!(x, y);
                    assert_ne!(ax, ay);
                    assert!(m.absmap().contains(&format!("ABS_X={x}")));
                    assert!(m.axismap().contains(&format!("={ax}")));
                }
            }
        }
        assert_eq!(
            Mapping::default().axismap(),
            "x1=ABS_X,y1^resp:32767:0:-32768=ABS_Y"
        );
    }
    #[test]
    fn rejects_commands_duplicates_and_invalid_flags() {
        for text in [
            "click a;touch /tmp/never",
            "stick middle",
            "invert_x true",
            "swap 2",
            "click a\nclick b",
            "unknown 0",
        ] {
            assert!(Mapping::parse(text).is_err(), "{text}");
        }
        assert!(Mapping::parse(&" ".repeat(513)).is_err());
    }
}
