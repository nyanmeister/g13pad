// SPDX-License-Identifier: GPL-3.0-or-later
//! Reusable LCD text, rasterized using the same font engine as the editor, without a display.
use crate::lcd::{self, Animation, Bitmap, H, W};
use crate::text_options::{Align, Options};
use egui::{Color32, FontData, FontFamily, FontId};
use std::{fs, time::Duration};

#[cfg(test)]
pub fn render(text: &str) -> Result<Animation, String> {
    render_with(text, &Options::default())
}

pub fn render_with(text: &str, options: &Options) -> Result<Animation, String> {
    options.validate()?;
    if text.len() > 4096 || text.chars().count() > 128 {
        return Err("LCD text is limited to 128 characters".into());
    }
    let text = text
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() || text.chars().any(|c| c.is_control() && c != '\n') {
        return Err("enter printable LCD text".into());
    }
    let mut defs = crate::fonts::with_characters(&text);
    let paths = options.font.clone().map(|p| vec![p]).unwrap_or_else(|| {
        vec![
            "/usr/share/fonts/TTF/RobotoMono-Regular.ttf".into(),
            "/usr/share/fonts/TTF/RobotoMonoNerdFontMono-Regular.ttf".into(),
        ]
    });
    for path in paths {
        match fs::metadata(&path) {
            Ok(m) if m.is_file() && m.len() <= 16 << 20 => {}
            _ if options.font.is_none() => continue,
            _ => {
                return Err(format!(
                    "{}: font must be a regular file under 16 MiB",
                    path.display()
                ))
            }
        }
        match fs::read(&path) {
            Ok(bytes) => {
                read_fonts::FontRef::from_index(&bytes, 0)
                    .map_err(|e| format!("{}: invalid font: {e}", path.display()))?;
                defs.font_data
                    .insert("lcd-mono".into(), FontData::from_owned(bytes).into());
                defs.families
                    .get_mut(&FontFamily::Monospace)
                    .unwrap()
                    .insert(0, "lcd-mono".into());
                break;
            }
            Err(_) if options.font.is_none() => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let ctx = egui::Context::default();
    ctx.set_fonts(defs);
    ctx.begin_pass(Default::default());
    let (galley, atlas) = ctx.fonts_mut(|fonts| {
        let mut job = egui::text::LayoutJob::simple(
            text,
            FontId::monospace(options.size),
            Color32::WHITE,
            if options.wrap {
                W as f32
            } else {
                f32::INFINITY
            },
        );
        job.halign = match options.align {
            Align::Left => egui::Align::LEFT,
            Align::Center => egui::Align::Center,
            Align::Right => egui::Align::RIGHT,
        };
        let g = fonts.layout_job(job);
        (g, fonts.image())
    });
    let mut output = ctx.end_pass();
    // This renderer consumes the atlas directly and has no GPU texture manager.
    output.textures_delta.clear();
    let bounds = galley.mesh_bounds;
    if !bounds.is_finite() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return Err("text has no visible glyphs".into());
    }
    let width = bounds.width().ceil() as u32;
    let height = bounds.height().ceil() as u32;
    let mut strip = image::GrayImage::new(width, height);
    for row in &galley.rows {
        for glyph in &row.glyphs {
            let uv = glyph.uv_rect;
            let pos = row.pos + glyph.pos.to_vec2() + uv.offset - bounds.min.to_vec2();
            for sy in uv.min[1]..uv.max[1] {
                for sx in uv.min[0]..uv.max[0] {
                    let x = pos.x.round() as i32 + i32::from(sx - uv.min[0]);
                    let y = pos.y.round() as i32 + i32::from(sy - uv.min[1]);
                    if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                        let alpha = atlas[(sx as usize, sy as usize)].a();
                        let pixel = strip.get_pixel_mut(x as u32, y as u32);
                        pixel.0[0] = pixel.0[0].max(alpha);
                    }
                }
            }
        }
    }
    if height > H as u32 {
        strip = image::imageops::resize(
            &strip,
            (width * H as u32 / height).max(1),
            H as u32,
            image::imageops::FilterType::Triangle,
        );
    }
    Ok(scroll_with(&strip, options))
}

/// Short text is centred. Longer text wraps with a half-panel gap, two pixels per 50 ms.
#[cfg(test)]
fn scroll(strip: &image::GrayImage) -> Animation {
    scroll_with(strip, &Options::default())
}

fn scroll_with(strip: &image::GrayImage, options: &Options) -> Animation {
    let width = strip.width() as usize;
    let height = strip.height() as usize;
    let scrolling = width > W && options.speed > 0;
    let span = width + W / 2;
    let mut frames = vec![];
    let step = ((options.speed as usize + 10) / 20).max(1);
    let delay = if scrolling {
        Duration::from_millis((step as f64 * 1000.0 / options.speed as f64).round() as u64)
    } else {
        Duration::ZERO
    };
    let pad = match options.align {
        Align::Left => 0,
        Align::Center => W.saturating_sub(width) / 2,
        Align::Right => W.saturating_sub(width),
    };
    let clip = match options.align {
        Align::Left => 0,
        Align::Center => width.saturating_sub(W) / 2,
        Align::Right => width.saturating_sub(W),
    };
    for offset in (0..if scrolling { span } else { 1 }).step_by(step) {
        let mut bm = Bitmap::blank();
        for y in 0..height.min(H) {
            for x in 0..W {
                let sx = if scrolling {
                    (x + offset) % span
                } else {
                    x.wrapping_sub(pad).wrapping_add(clip)
                };
                if sx < width && strip.get_pixel(sx as u32, y as u32).0[0] >= 128 {
                    bm.set(x, y + (H - height.min(H)) / 2, true);
                }
            }
        }
        if options.invert {
            for y in 0..H {
                for x in 0..W {
                    bm.set(x, y, !bm.get(x, y));
                }
            }
        }
        frames.push((delay, bm));
    }
    Animation { frames }
}

pub fn keep(text: &str, name: Option<&str>) -> Result<String, String> {
    keep_with(text, name, &Options::default())
}

pub fn keep_with(text: &str, name: Option<&str>, options: &Options) -> Result<String, String> {
    let name = name.unwrap_or("text");
    if name.is_empty()
        || name.chars().count() > 64
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || " -_".contains(c))
    {
        return Err(
            "LCD text name must contain letters, digits, spaces, - or _ (up to 64 characters)"
                .into(),
        );
    }
    let name = if lcd::path(name).exists() {
        lcd::fork_name(name)
    } else {
        name.to_string()
    };
    let anim = render_with(text, options)?;
    lcd::keep_animation(&name, &anim)?;
    fs::write(lcd::dir().join(format!("{name}.text")), text).map_err(|e| e.to_string())?;
    fs::write(
        lcd::dir().join(format!("{name}.textopts")),
        options.to_text(),
    )
    .map_err(|e| e.to_string())?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_renders_without_a_display_and_keeps_reusable_frames() {
        let _sandbox = crate::test_support::Sandbox::new("marquee");
        let short = render("G13").unwrap();
        assert_eq!(short.frames.len(), 1);
        assert!(short.first().lit() > 100);
        let long = render("Error: profile unavailable").unwrap();
        assert!(long.animated() && long.frames.len() < 2000);
        assert_eq!(
            long.frames.iter().map(|(d, _)| *d).sum::<Duration>(),
            Duration::from_millis(long.frames.len() as u64 * 50)
        );
        assert_ne!(long.frames[0].1, long.frames[1].1);
        let name = keep("Error: profile unavailable", Some("error")).unwrap();
        assert_eq!(lcd::load_anim(&name).unwrap().to_bytes(), long.to_bytes());
        assert_eq!(lcd::load(&name).unwrap(), *long.first());
        assert!(lcd::source(&name).is_none());
        assert_ne!(keep("G13", Some("error")).unwrap(), name);
        for bad in ["", "\n\t", "bad\0text", "\u{200b}"] {
            assert!(render(bad).is_err(), "{bad:?}");
        }
        assert!(render(&"a".repeat(129)).is_err());
        assert!(keep("hello", Some("../escape")).is_err());
    }
    #[test]
    fn scrolling_uses_bright_bits_and_a_gap_without_clipping() {
        let strip = image::GrayImage::from_pixel(162, 43, image::Luma([255]));
        let a = scroll(&strip);
        assert_eq!(a.frames.len(), 121);
        assert_eq!(a.first().lit(), W * H);
        assert_eq!(a.frames[81].1.lit(), 80 * H);
        assert!(a
            .frames
            .iter()
            .all(|(_, bm)| bm.0[W * 5..].iter().all(|b| b & 0xf8 == 0)));
    }
    #[test]
    fn text_settings_change_rendering_and_survive_reload() {
        let _sandbox = crate::test_support::Sandbox::new("text-settings");
        let options = Options {
            size: 16.0,
            wrap: true,
            align: Align::Right,
            speed: 0,
            ..Options::default()
        };
        let lines = render_with("G13\nTwo lines", &options).unwrap();
        assert_eq!(lines.frames.len(), 1);
        assert_ne!(
            lines.first(),
            render_with("G13 Two lines", &options).unwrap().first()
        );
        assert_ne!(
            lines.first(),
            render_with(
                "G13\nTwo lines",
                &Options {
                    size: 10.0,
                    ..options.clone()
                }
            )
            .unwrap()
            .first()
        );
        let inverted = render_with(
            "G13\nTwo lines",
            &Options {
                invert: true,
                ..options.clone()
            },
        )
        .unwrap();
        for y in 0..H {
            for x in 0..W {
                assert_ne!(lines.first().get(x, y), inverted.first().get(x, y));
            }
        }
        let name = keep_with("G13\nTwo lines", Some("custom"), &options).unwrap();
        assert_eq!(Options::load(&name), options);
        assert_eq!(lcd::load(&name).unwrap(), *lines.first());
        assert!(lcd::load_anim(&name).is_none());
        assert_eq!(Options::load("legacy"), Options::default());
        let phrase = "A long phrase that should wrap over several lines";
        assert!(!render_with(
            phrase,
            &Options {
                speed: 40,
                ..options.clone()
            }
        )
        .unwrap()
        .animated());
        assert!(render_with(
            phrase,
            &Options {
                wrap: false,
                speed: 40,
                ..options.clone()
            }
        )
        .unwrap()
        .animated());
        assert!(render_with(
            "G13",
            &Options {
                font: Some("/dev/null".into()),
                ..options.clone()
            }
        )
        .is_err());
        let invalid = lcd::dir().join("invalid.ttf");
        fs::write(&invalid, b"not a font").unwrap();
        assert!(render_with(
            "G13",
            &Options {
                font: Some(invalid),
                ..options
            }
        )
        .is_err());
    }
    #[test]
    fn alignment_still_clipping_and_speed_have_defined_pixels() {
        let short = image::GrayImage::from_pixel(2, 1, image::Luma([255]));
        for (align, start) in [(Align::Left, 0), (Align::Center, 79), (Align::Right, 158)] {
            let a = scroll_with(
                &short,
                &Options {
                    align,
                    ..Options::default()
                },
            );
            assert!(a.first().get(start, 21));
            assert!(a.first().get(start + 1, 21));
            assert_eq!(a.first().lit(), 2);
        }
        let wide =
            image::GrayImage::from_fn(200, 1, |x, _| image::Luma([if x >= 160 { 255 } else { 0 }]));
        let left = scroll_with(
            &wide,
            &Options {
                speed: 0,
                align: Align::Left,
                ..Options::default()
            },
        );
        let right = scroll_with(
            &wide,
            &Options {
                speed: 0,
                align: Align::Right,
                ..Options::default()
            },
        );
        assert_eq!(left.frames.len(), 1);
        assert_eq!(left.first().lit(), 0);
        assert_eq!(right.first().lit(), 40);
        let fast = scroll_with(
            &wide,
            &Options {
                speed: 120,
                ..Options::default()
            },
        );
        assert_eq!(fast.frames[0].0, Duration::from_millis(50));
        assert_eq!(fast.frames.len(), 47);
    }
}
