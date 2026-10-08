// SPDX-License-Identifier: GPL-3.0-or-later
//! Append an installed font for characters absent from egui's bundled Latin fonts.
use egui::{FontData, FontDefinitions, FontFamily};
use std::{fs, io::Read, process::Command};

pub fn with_characters(text: &str) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut chars: Vec<_> = text
        .chars()
        .take(256)
        .filter(|c| !c.is_ascii() && !c.is_control())
        .collect();
    chars.sort_unstable();
    chars.dedup();
    if chars.is_empty() {
        return fonts;
    }
    let charset = chars
        .iter()
        .map(|c| format!("{:x}", *c as u32))
        .collect::<Vec<_>>()
        .join(" ");
    let Ok(output) = Command::new("/usr/bin/timeout")
        .args([
            "1",
            "fc-match",
            "-f",
            "%{file}\n",
            &format!(":charset={charset}"),
        ])
        .output()
    else {
        return fonts;
    };
    if !output.status.success() {
        return fonts;
    }
    let Ok(path) = std::str::from_utf8(&output.stdout) else {
        return fonts;
    };
    let Ok(file) = fs::File::open(path.trim()) else {
        return fonts;
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= 16 << 20)
    {
        return fonts;
    }
    let mut bytes = Vec::new();
    if file.take((16 << 20) + 1).read_to_end(&mut bytes).is_err()
        || bytes.len() > 16 << 20
        || read_fonts::FontRef::from_index(&bytes, 0).is_err()
    {
        return fonts;
    }
    fonts
        .font_data
        .insert("layout-fallback".into(), FontData::from_owned(bytes).into());
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("layout-fallback".into());
    }
    fonts
}
