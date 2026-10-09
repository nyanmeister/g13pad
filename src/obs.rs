// SPDX-License-Identifier: GPL-3.0-or-later
//! The OBS overlay (`g13map obs`): a borderless, transparent window that draws the pad
//! with its keys lit as they are pressed, for OBS to capture like any other window.
//!
//! The picture is a sprite sheet in the shape of the input-overlay plugin's presets
//! (`assets/obs/g13.png` with `g13.json`: a body texture, then one sprite per key with
//! its pressed twin 3 px below), generated here from the editor's board geometry on a
//! 4-pixel grid, in the style of the plugin's pixel keyboard. `--dump DIR` writes the
//! sheet out for repainting; `--asset DIR` (or `~/.config/g13map/obs/`) loads one.
//!
//! State comes from the daemon's key state file (`/run/g13d/g13-0_keys`: `stick X Y`
//! and `keys NAME...`, rewritten on change), polled a hundred times a second. The
//! plugin itself cannot drive this: its gamepad path stops at the 21 mapped buttons
//! and its keyboard path sees the profile's bindings, not the keys.
use crate::{
    board,
    glass::{Glass, Rgb},
    lcd,
};
use egui::{Pos2, Vec2};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

/// One pixel of the art is this many image pixels.
pub const UNIT: i32 = 4;
/// Sheet width: the body at the left, the key sprites shelved to its right.
const SHEET_W: i32 = 1024;
/// The pressed sprite sits this far below its key, as in the plugin's presets.
const PRESSED_GAP: u32 = 3;
/// How far the stick cap travels at full deflection, in pixels.
const STICK_TRAVEL: f32 = 12.0;
pub const PNG: &str = "g13.png";
pub const JSON: &str = "g13.json";

type Rgba = [u8; 4];
const CLEAR: Rgba = [0, 0, 0, 0];
const OUTLINE: Rgba = [31, 31, 31, 255];
const BODY: Rgba = [44, 44, 44, 255];
const SCREEN: Rgba = [36, 40, 42, 255];
const KEY: Rgba = [99, 99, 99, 255];
const HI: Rgba = [135, 135, 135, 255];
const LO: Rgba = [77, 77, 77, 255];
const GLYPH: Rgba = [200, 200, 200, 255];
const PRESSED_KEY: Rgba = [52, 52, 52, 255];
const PRESSED_HI: Rgba = [61, 61, 61, 255];
const PRESSED_LO: Rgba = [46, 46, 46, 255];
const CYAN: Rgba = [0, 195, 255, 255];

// ---- the sheet: a raster drawn in units ----

struct Sheet {
    w: i32,
    h: i32,
    px: Vec<Rgba>,
}

impl Sheet {
    fn new(w: i32, h: i32) -> Sheet {
        Sheet {
            w,
            h,
            px: vec![CLEAR; (w * h) as usize],
        }
    }
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.px[(yy * self.w + xx) as usize] = c;
            }
        }
    }
    #[allow(clippy::too_many_arguments)] // x y, then the unit rectangle, then the colour
    /// A rectangle of art pixels at an image pixel origin.
    fn units(&mut self, x: i32, y: i32, ux: i32, uy: i32, uw: i32, uh: i32, c: Rgba) {
        self.fill(x + ux * UNIT, y + uy * UNIT, uw * UNIT, uh * UNIT, c);
    }
    fn unit(&mut self, x: i32, y: i32, ux: i32, uy: i32, c: Rgba) {
        self.units(x, y, ux, uy, 1, 1, c);
    }
    fn png(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            bytes.extend_from_slice(p);
        }
        let img = image::RgbaImage::from_raw(self.w as u32, self.h as u32, bytes)
            .expect("sheet buffer matches its size");
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)
            .expect("PNG encoding of an in-memory image");
        out.into_inner()
    }
}

/// A 3x5 pixel font: digits and the letters on the pad.
fn glyph(c: char) -> Option<[&'static str; 5]> {
    Some(match c {
        '0' => ["###", "#.#", "#.#", "#.#", "###"],
        '1' => [".#.", "##.", ".#.", ".#.", "###"],
        '2' => ["###", "..#", "###", "#..", "###"],
        '3' => ["###", "..#", "###", "..#", "###"],
        '4' => ["#.#", "#.#", "###", "..#", "..#"],
        '5' => ["###", "#..", "###", "..#", "###"],
        '6' => ["###", "#..", "###", "#.#", "###"],
        '7' => ["###", "..#", "..#", "..#", "..#"],
        '8' => ["###", "#.#", "###", "#.#", "###"],
        '9' => ["###", "#.#", "###", "..#", "###"],
        'G' => ["###", "#..", "#.#", "#.#", "###"],
        'L' => ["#..", "#..", "#..", "#..", "###"],
        'M' => ["#.#", "###", "#.#", "#.#", "#.#"],
        'R' => ["##.", "#.#", "##.", "#.#", "#.#"],
        _ => return None,
    })
}

/// A key cap `uw` by `uh` units at image pixel (x, y): outline, a bevel (light top and
/// left, dark bottom and right), the label centred. `round` clips the corners for the
/// two round buttons.
#[allow(clippy::too_many_arguments)]
fn key_cap(
    s: &mut Sheet,
    x: i32,
    y: i32,
    uw: i32,
    uh: i32,
    label: &str,
    round: bool,
    pressed: bool,
) {
    let (cap, hi, lo, ink) = if pressed {
        (PRESSED_KEY, PRESSED_HI, PRESSED_LO, CYAN)
    } else {
        (KEY, HI, LO, GLYPH)
    };
    s.units(x, y, 0, 0, uw, uh, OUTLINE);
    s.units(x, y, 1, 1, uw - 2, uh - 2, cap);
    s.units(x, y, uw - 2, 1, 1, uh - 2, lo);
    s.units(x, y, 1, 1, 1, uh - 2, hi);
    s.units(x, y, 1, uh - 2, uw - 2, 1, lo);
    s.units(x, y, 1, 1, uw - 2, 1, hi);
    if round {
        for (ux, uy) in [(0, 0), (uw - 1, 0), (0, uh - 1), (uw - 1, uh - 1)] {
            s.unit(x, y, ux, uy, CLEAR);
        }
        s.unit(x, y, 1, 1, OUTLINE);
        s.unit(x, y, uw - 2, 1, OUTLINE);
        s.unit(x, y, 1, uh - 2, OUTLINE);
        s.unit(x, y, uw - 2, uh - 2, OUTLINE);
    }
    let glyphs: Vec<_> = label.chars().filter_map(glyph).collect();
    if glyphs.is_empty() {
        // No label to turn cyan: a pressed blank cap shows a bar instead.
        if pressed {
            let (bw, bh) = ((uw - 6).max(2), (uh - 6).max(2));
            s.units(x, y, (uw - bw) / 2, (uh - bh) / 2, bw, bh, ink);
        }
        return;
    }
    let text_w = glyphs.len() as i32 * 4 - 1;
    let x0 = (uw - text_w) / 2;
    let y0 = (uh - 5) / 2;
    for (i, g) in glyphs.iter().enumerate() {
        for (row, line) in g.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                if ch == '#' {
                    s.unit(x, y, x0 + i as i32 * 4 + col as i32, y0 + row as i32, ink);
                }
            }
        }
    }
}

/// The stick cap, a 6x6 pixel disc; pressed (the stick click) shows a cyan centre.
fn stick_cap(s: &mut Sheet, x: i32, y: i32, pressed: bool) {
    let (cap, hi) = if pressed {
        (PRESSED_KEY, PRESSED_HI)
    } else {
        (KEY, HI)
    };
    let mask = [".####.", "######", "######", "######", "######", ".####."];
    let on = |ux: i32, uy: i32| {
        (0..6).contains(&ux)
            && (0..6).contains(&uy)
            && mask[uy as usize].as_bytes()[ux as usize] == b'#'
    };
    for uy in 0..6 {
        for ux in 0..6 {
            if !on(ux, uy) {
                continue;
            }
            let edge = !(on(ux - 1, uy) && on(ux + 1, uy) && on(ux, uy - 1) && on(ux, uy + 1));
            s.unit(x, y, ux, uy, if edge { OUTLINE } else { cap });
        }
    }
    s.unit(x, y, 2, 1, hi);
    s.unit(x, y, 1, 2, hi);
    if pressed {
        s.units(x, y, 2, 2, 2, 2, CYAN);
    }
}

/// Even-odd point-in-polygon on the board outline, in board pixels.
fn inside(poly: &[[f32; 2]], x: f32, y: f32) -> bool {
    let mut hit = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = (poly[i][0], poly[i][1]);
        let (xj, yj) = (poly[j][0], poly[j][1]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

/// The body: the device silhouette with an outline, the LCD, the stick well.
fn body(s: &mut Sheet, uw: i32, uh: i32) {
    let poly: Vec<[f32; 2]> = board::OUTLINE
        .iter()
        .map(|p| [p[0] + board::OFF.x, p[1] + board::OFF.y])
        .collect();
    let filled = |ux: i32, uy: i32| {
        (0..uw).contains(&ux)
            && (0..uh).contains(&uy)
            && inside(
                &poly,
                (ux as f32 + 0.5) * UNIT as f32,
                (uy as f32 + 0.5) * UNIT as f32,
            )
    };
    for uy in 0..uh {
        for ux in 0..uw {
            if !filled(ux, uy) {
                continue;
            }
            let edge = !(filled(ux - 1, uy)
                && filled(ux + 1, uy)
                && filled(ux, uy - 1)
                && filled(ux, uy + 1));
            s.unit(0, 0, ux, uy, if edge { OUTLINE } else { BODY });
        }
    }
    let (lx, ly, lw, lh) = screen();
    s.units(0, 0, lx, ly, lw, lh, OUTLINE);
    s.units(0, 0, lx + 1, ly + 1, lw - 2, lh - 2, LO);
    s.units(0, 0, lx + 2, ly + 2, lw - 4, lh - 4, SCREEN);
    let centre = board::STICK_CENTRE + board::OFF;
    let (cx, cy) = (centre.x / UNIT as f32, centre.y / UNIT as f32);
    let well = |ux: i32, uy: i32| {
        let (dx, dy) = (ux as f32 + 0.5 - cx, uy as f32 + 0.5 - cy);
        (dx * dx + dy * dy).sqrt() <= 9.0
    };
    for uy in 0..uh {
        for ux in 0..uw {
            if !well(ux, uy) {
                continue;
            }
            let edge =
                !(well(ux - 1, uy) && well(ux + 1, uy) && well(ux, uy - 1) && well(ux, uy + 1));
            s.unit(0, 0, ux, uy, if edge { OUTLINE } else { PRESSED_KEY });
        }
    }
}

fn to_unit(v: f32) -> i32 {
    (v / UNIT as f32).round() as i32
}

/// The LCD frame on the body, in units: x, y, width, height (outline included).
fn screen() -> (i32, i32, i32, i32) {
    let lcd = board::LCD.translate(board::OFF);
    let (lx, ly) = (to_unit(lcd.min.x), to_unit(lcd.min.y));
    (lx, ly, to_unit(lcd.max.x) - lx, to_unit(lcd.max.y) - ly)
}

/// Where the live LCD picture goes, in overlay pixels: 1:1, centred on the screen.
fn lcd_pos() -> [i32; 2] {
    let (lx, ly, lw, lh) = screen();
    [
        (lx + 2) * UNIT + ((lw - 4) * UNIT - lcd::W as i32) / 2,
        (ly + 2) * UNIT + ((lh - 4) * UNIT - lcd::H as i32) / 2,
    ]
}

/// A control to draw: where it sits on the overlay (units) and how big its cap is.
struct Control {
    id: String,
    ux: i32,
    uy: i32,
    uw: i32,
    uh: i32,
    label: String,
    round: bool,
    stick: bool,
}

/// The pad's controls from the editor's board, snapped to the grid. Thin keys (the LCD
/// and mode keys) grow to hold a label; everything else keeps the photo's proportions.
fn controls() -> Vec<Control> {
    let mut v = Vec::new();
    for spot in board::spots() {
        let name = spot.name;
        let centre = spot.centre + board::OFF;
        let (uw, uh, label, round, stick) = match spot.kind {
            board::Kind::Zone => continue,
            board::Kind::Top => (6, 6, String::new(), false, true),
            _ => match name {
                "BD" | "LIGHT" => (6, 6, String::new(), true, false),
                "L1" | "L2" | "L3" | "L4" => (11, 7, name.to_string(), false, false),
                "M1" | "M2" | "M3" | "MR" => (16, 7, name.to_string(), false, false),
                "LEFT" | "DOWN" => (
                    to_unit(spot.size.x),
                    to_unit(spot.size.y),
                    String::new(),
                    false,
                    false,
                ),
                _ => (
                    to_unit(spot.size.x),
                    to_unit(spot.size.y),
                    name.trim_start_matches('G').to_string(),
                    false,
                    false,
                ),
            },
        };
        v.push(Control {
            id: name.to_string(),
            ux: (centre.x / UNIT as f32 - uw as f32 / 2.0).round() as i32,
            uy: (centre.y / UNIT as f32 - uh as f32 / 2.0).round() as i32,
            uw,
            uh,
            label,
            round,
            stick,
        });
    }
    v
}

/// The built-in sheet and layout: the PNG bytes and the JSON text.
pub fn generate() -> (Vec<u8>, String) {
    let (uw, uh) = (to_unit(board::W + 1.0), to_unit(board::H));
    let (body_w, body_h) = (uw * UNIT, uh * UNIT);
    let controls = controls();
    // Shelf-pack the sprites to the right of the body: each cell holds a cap and its
    // pressed twin below.
    let x0 = body_w + UNIT;
    let (mut x, mut y, mut shelf) = (x0, 0, 0);
    let mut cells = Vec::new();
    for c in &controls {
        let (w, h) = (c.uw * UNIT, c.uh * UNIT * 2 + PRESSED_GAP as i32);
        if x + w > SHEET_W {
            x = x0;
            y += shelf + UNIT;
            shelf = 0;
        }
        cells.push((x, y));
        x += w + UNIT;
        shelf = shelf.max(h);
    }
    let sheet_h = body_h.max(y + shelf);
    let mut s = Sheet::new(SHEET_W, sheet_h);
    body(&mut s, uw, uh);
    let mut elements = vec![
        serde_json::json!({
            "id": "body", "type": 0, "z_level": 0,
            "pos": [0, 0], "mapping": [0, 0, body_w, body_h],
        }),
        // The live LCD frame, drawn by the overlay from the daemon's file (no sprite).
        serde_json::json!({
            "id": "lcd", "type": 0, "z_level": 1,
            "pos": lcd_pos(), "mapping": [0, 0, lcd::W, lcd::H],
        }),
    ];
    for (c, (sx, sy)) in controls.iter().zip(&cells) {
        let (w, h) = (c.uw * UNIT, c.uh * UNIT);
        for (pressed, dy) in [(false, 0), (true, h + PRESSED_GAP as i32)] {
            if c.stick {
                stick_cap(&mut s, *sx, sy + dy, pressed);
            } else {
                key_cap(&mut s, *sx, sy + dy, c.uw, c.uh, &c.label, c.round, pressed);
            }
        }
        let pos = [c.ux * UNIT, c.uy * UNIT];
        let mapping = [*sx, *sy, w, h];
        elements.push(if c.stick {
            serde_json::json!({
                "id": "stick", "type": 5, "z_level": 2, "side": 0,
                "stick_radius": STICK_TRAVEL, "pos": pos, "mapping": mapping,
            })
        } else {
            serde_json::json!({
                "id": c.id, "type": 1, "z_level": 1, "code": 0,
                "pos": pos, "mapping": mapping,
            })
        });
    }
    let layout = serde_json::json!({
        "default_width": 0, "default_height": 0, "space_h": 0, "space_v": 0, "flags": 0,
        "overlay_width": body_w, "overlay_height": body_h,
        "elements": elements,
    });
    let mut json = serde_json::to_string_pretty(&layout).expect("layout serializes");
    json.push('\n');
    (s.png(), json)
}

// ---- the asset as loaded ----

pub struct Element {
    pub id: String,
    pub kind: u64,
    pub z: i64,
    pub pos: [f32; 2],
    pub mapping: [f32; 4],
    pub radius: f32,
}

pub struct Asset {
    pub width: f32,
    pub height: f32,
    pub elements: Vec<Element>,
    pub image: image::RgbaImage,
}

impl Asset {
    pub fn parse(png: &[u8], json: &str) -> Result<Asset, String> {
        let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|e| format!("{PNG}: {e}"))?
            .into_rgba8();
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("{JSON}: {e}"))?;
        let num = |v: &serde_json::Value, key: &str| -> Result<f32, String> {
            v.get(key)
                .and_then(serde_json::Value::as_f64)
                .map(|n| n as f32)
                .ok_or_else(|| format!("{JSON}: missing number {key}"))
        };
        let mut elements = Vec::new();
        for e in v
            .get("elements")
            .and_then(serde_json::Value::as_array)
            .ok_or("elements missing")?
        {
            let list = |key: &str, n: usize| -> Result<Vec<f32>, String> {
                let a = e
                    .get(key)
                    .and_then(serde_json::Value::as_array)
                    .ok_or(format!("{JSON}: element without {key}"))?;
                if a.len() != n {
                    return Err(format!("{JSON}: {key} needs {n} numbers"));
                }
                a.iter()
                    .map(|x| {
                        x.as_f64()
                            .map(|n| n as f32)
                            .ok_or(format!("{JSON}: {key} is not numeric"))
                    })
                    .collect()
            };
            let pos = list("pos", 2)?;
            let mapping = list("mapping", 4)?;
            elements.push(Element {
                id: e
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                kind: e
                    .get("type")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                z: e.get("z_level")
                    .and_then(|z| {
                        z.as_i64()
                            .or_else(|| z.as_str().and_then(|s| s.parse().ok()))
                    })
                    .unwrap_or(0),
                pos: [pos[0], pos[1]],
                mapping: [mapping[0], mapping[1], mapping[2], mapping[3]],
                radius: e
                    .get("stick_radius")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(STICK_TRAVEL as f64) as f32,
            });
        }
        elements.sort_by_key(|e| e.z);
        Ok(Asset {
            width: num(&v, "overlay_width")?,
            height: num(&v, "overlay_height")?,
            elements,
            image,
        })
    }

    fn load(dir: &Path) -> Result<Asset, String> {
        let png =
            fs::read(dir.join(PNG)).map_err(|e| format!("{}: {e}", dir.join(PNG).display()))?;
        let json = fs::read_to_string(dir.join(JSON))
            .map_err(|e| format!("{}: {e}", dir.join(JSON).display()))?;
        Asset::parse(&png, &json)
    }

    /// `--asset DIR`, else a repainted copy under the config directory, else the built-in.
    fn find(dir: Option<&Path>) -> Result<Asset, String> {
        if let Some(dir) = dir {
            return Asset::load(dir);
        }
        let own = crate::config_dir().join("obs");
        if own.join(PNG).is_file() && own.join(JSON).is_file() {
            return Asset::load(&own);
        }
        let (png, json) = generate();
        Asset::parse(&png, &json)
    }
}

/// Writes the built-in sheet and layout into `dir`.
pub fn dump(dir: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (png, json) = generate();
    fs::write(dir.join(PNG), png).map_err(|e| format!("{}: {e}", dir.join(PNG).display()))?;
    fs::write(dir.join(JSON), json).map_err(|e| format!("{}: {e}", dir.join(JSON).display()))?;
    Ok(format!("wrote {} and {} to {}", PNG, JSON, dir.display()))
}

// ---- the daemon's key state ----

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub keys: HashSet<String>,
    /// Raw stick bytes, 0..255 each, centre about 128.
    pub stick: (u8, u8),
    pub backlight: Rgb,
    /// The frame on the glass, when the daemon has written one.
    pub lcd: Option<lcd::Bitmap>,
    /// When `~/.config/g13map/glass` last changed: a window redraws through the new table.
    pub glass_stamp: Option<std::time::SystemTime>,
}

impl Default for State {
    fn default() -> Self {
        State {
            keys: HashSet::new(),
            stick: (128, 128),
            backlight: [0, 0, 255],
            lcd: None,
            glass_stamp: None,
        }
    }
}

impl State {
    /// The keys file: `stick X Y`, `backlight R G B`, `keys NAME...`.
    pub fn parse(text: &str) -> State {
        let mut s = State::default();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let numbers = |words: &mut std::str::SplitWhitespace| -> Vec<u8> {
                words.filter_map(|w| w.parse().ok()).collect()
            };
            match words.next() {
                Some("stick") => {
                    if let [x, y] = numbers(&mut words)[..] {
                        s.stick = (x, y);
                    }
                }
                Some("backlight") => {
                    if let [r, g, b] = numbers(&mut words)[..] {
                        s.backlight = [r, g, b];
                    }
                }
                Some("keys") => s.keys = words.map(str::to_string).collect(),
                _ => {}
            }
        }
        s
    }
}

/// `/run/g13d/g13-0_keys` and `g13-0_lcd`: the daemon's state files, named after its
/// command pipe.
pub fn state_path(suffix: &str) -> PathBuf {
    let pipe = crate::daemon::pipe_path();
    let name = pipe.file_name().and_then(|n| n.to_str()).unwrap_or("g13-0");
    pipe.with_file_name(format!("{name}_{suffix}"))
}

// ---- the window ----

struct Overlay {
    asset: Asset,
    texture: egui::TextureHandle,
    /// The LCD frame as the glass shows it, re-uploaded when the frame or colour changes.
    lcd_texture: egui::TextureHandle,
    lcd_shown: Option<(lcd::Bitmap, Rgb)>,
    glass: Glass,
    glass_stamp: Option<std::time::SystemTime>,
    state: Arc<Mutex<State>>,
    scale: f32,
    background: [f32; 4],
    /// `--lcd`: the window is the LCD alone, at this many pixels per LCD pixel.
    lcd_only: Option<f32>,
}

impl Overlay {
    fn refresh_lcd(&mut self, state: &State) {
        if state.glass_stamp != self.glass_stamp {
            self.glass_stamp = state.glass_stamp;
            self.glass = Glass::load();
            self.lcd_shown = None;
        }
        let Some(frame) = &state.lcd else { return };
        if self
            .lcd_shown
            .as_ref()
            .is_some_and(|(f, c)| f == frame && *c == state.backlight)
        {
            return;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [lcd::W, lcd::H],
            &self.glass.render(frame, state.backlight),
        );
        self.lcd_texture.set(image, egui::TextureOptions::NEAREST);
        self.lcd_shown = Some((frame.clone(), state.backlight));
    }
}

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.background
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        self.refresh_lcd(&state);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let origin = ui.max_rect().min;
                let full = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                if let Some(px) = self.lcd_only {
                    if self.lcd_shown.is_some() {
                        let rect = egui::Rect::from_min_size(
                            origin,
                            Vec2::new(lcd::W as f32, lcd::H as f32) * px,
                        );
                        ui.painter()
                            .image(self.lcd_texture.id(), rect, full, egui::Color32::WHITE);
                    }
                    return;
                }
                let (sheet_w, sheet_h) = (
                    self.asset.image.width() as f32,
                    self.asset.image.height() as f32,
                );
                for e in &self.asset.elements {
                    let [sx, sy, w, h] = e.mapping;
                    if e.id == "lcd" {
                        if self.lcd_shown.is_some() {
                            let rect = egui::Rect::from_min_size(
                                origin + Vec2::new(e.pos[0], e.pos[1]) * self.scale,
                                Vec2::new(w, h) * self.scale,
                            );
                            ui.painter().image(
                                self.lcd_texture.id(),
                                rect,
                                full,
                                egui::Color32::WHITE,
                            );
                        }
                        continue;
                    }
                    let pressed = match e.kind {
                        1 => state.keys.contains(&e.id),
                        5 => state.keys.contains("TOP"),
                        _ => false,
                    };
                    let sy = if pressed {
                        sy + h + PRESSED_GAP as f32
                    } else {
                        sy
                    };
                    let mut pos = Pos2::new(e.pos[0], e.pos[1]);
                    if e.kind == 5 {
                        pos += Vec2::new(
                            (state.stick.0 as f32 - 127.5) / 127.5 * e.radius,
                            (state.stick.1 as f32 - 127.5) / 127.5 * e.radius,
                        );
                    }
                    let rect = egui::Rect::from_min_size(
                        origin + pos.to_vec2() * self.scale,
                        Vec2::new(w, h) * self.scale,
                    );
                    let uv = egui::Rect::from_min_max(
                        Pos2::new(sx / sheet_w, sy / sheet_h),
                        Pos2::new((sx + w) / sheet_w, (sy + h) / sheet_h),
                    );
                    ui.painter()
                        .image(self.texture.id(), rect, uv, egui::Color32::WHITE);
                }
            });
    }
}

/// Polls the daemon's state files (and the glass table, four times a second); a change
/// repaints.
pub(crate) fn watch(state: Arc<Mutex<State>>, ctx: egui::Context) {
    let (keys, lcd) = (state_path("keys"), state_path("lcd"));
    thread::spawn(move || {
        let (mut last_keys, mut last_lcd) = (String::new(), Vec::new());
        let mut stamp = Glass::stamp();
        let mut tick = 0u32;
        loop {
            let text = fs::read_to_string(&keys).unwrap_or_default();
            let frame = fs::read(&lcd).unwrap_or_default();
            tick = tick.wrapping_add(1);
            let restamp = tick.is_multiple_of(25) && {
                let now = Glass::stamp();
                let changed = now != stamp;
                stamp = now;
                changed
            };
            if text != last_keys || frame != last_lcd || restamp {
                last_keys = text;
                last_lcd = frame;
                if let Ok(mut s) = state.lock() {
                    *s = State::parse(&last_keys);
                    s.lcd = lcd::Bitmap::from_lpbm(&last_lcd).ok();
                    s.glass_stamp = stamp;
                }
                ctx.request_repaint();
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
}

pub const USAGE: &str =
    "usage: g13map obs [--asset DIR] [--scale N] [--background RRGGBB] [--lcd [N]] [--dump DIR]
  --asset DIR        draw a repainted sheet (g13.png + g13.json) instead of the built-in one
                     (~/.config/g13map/obs/ is used when it holds both files)
  --scale N          integer magnification of the window (default 1; OBS scales too)
  --background RRGGBB  an opaque window colour to chroma-key, instead of transparency
  --lcd [N]          the LCD alone, N pixels per LCD pixel (default 4): a second window,
                     \"G13 LCD\", to place and scale on its own
  --dump DIR         write the built-in sheet and layout to DIR for repainting, and exit
  --calibrate        instead, the window that matches the glass's colours by eye (g13map glass)
The window is borderless, titled \"G13 overlay\" (class g13map-obs), for an OBS window
capture; keys light as the pad reports them, whatever the active profile binds, the
stick cap travels, and the LCD shows the frame on the glass in the backlight's colour
(translated by ~/.config/g13map/glass).";

pub fn run(args: &[String]) -> Result<String, String> {
    let mut asset_dir = None;
    let mut scale = 1.0f32;
    let mut background = [0.0, 0.0, 0.0, 0.0];
    let mut lcd_only = None;
    fn value<'a>(
        it: &mut impl Iterator<Item = &'a String>,
        arg: &str,
    ) -> Result<&'a String, String> {
        it.next().ok_or(format!("{arg} needs a value"))
    }
    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" => return Ok(format!("g13pad {} (g13map-obs)", env!("CARGO_PKG_VERSION"))),
            "--help" | "-h" => return Ok(USAGE.into()),
            "--calibrate" => return crate::calibrate::run(),
            "--dump" => return dump(Path::new(value(&mut it, arg)?)),
            "--asset" => asset_dir = Some(PathBuf::from(value(&mut it, arg)?)),
            "--scale" => {
                scale = value(&mut it, arg)?
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=8).contains(n))
                    .ok_or("--scale takes 1 to 8")? as f32
            }
            "--lcd" => {
                let n = match it.peek().and_then(|w| w.parse::<u32>().ok()) {
                    Some(n) => {
                        it.next();
                        n
                    }
                    None => UNIT as u32,
                };
                lcd_only = Some(
                    (1..=16)
                        .contains(&n)
                        .then_some(n as f32)
                        .ok_or("--lcd takes 1 to 16")?,
                );
            }
            "--background" => {
                let hex = value(&mut it, arg)?;
                let n = u32::from_str_radix(hex, 16)
                    .ok()
                    .filter(|_| hex.len() == 6)
                    .ok_or("--background takes RRGGBB")?;
                background = [
                    ((n >> 16) & 255) as f32 / 255.0,
                    ((n >> 8) & 255) as f32 / 255.0,
                    (n & 255) as f32 / 255.0,
                    1.0,
                ];
            }
            _ => return Err(USAGE.into()),
        }
    }
    let asset = Asset::find(asset_dir.as_deref())?;
    let size = match lcd_only {
        Some(px) => [lcd::W as f32 * px, lcd::H as f32 * px],
        None => [asset.width * scale, asset.height * scale],
    };
    let title = if lcd_only.is_some() {
        "G13 LCD"
    } else {
        "G13 overlay"
    };
    let state = Arc::new(Mutex::new(State::default()));
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(background[3] == 0.0)
            .with_app_id("g13map-obs")
            .with_title(title),
        ..Default::default()
    };
    let watched = state.clone();
    eframe::run_native(
        "g13map-obs",
        opts,
        Box::new(move |cc| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [asset.image.width() as usize, asset.image.height() as usize],
                asset.image.as_raw(),
            );
            let texture =
                cc.egui_ctx
                    .load_texture("g13-sheet", image, egui::TextureOptions::NEAREST);
            let blank = egui::ColorImage::filled([lcd::W, lcd::H], egui::Color32::TRANSPARENT);
            let lcd_texture =
                cc.egui_ctx
                    .load_texture("g13-lcd", blank, egui::TextureOptions::NEAREST);
            watch(watched, cc.egui_ctx.clone());
            Ok(Box::new(Overlay {
                asset,
                texture,
                lcd_texture,
                lcd_shown: None,
                glass: Glass::load(),
                glass_stamp: Glass::stamp(),
                state,
                scale,
                background,
                lcd_only,
            }))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_parses_the_daemon_file() {
        let s = State::parse("stick 0 255\nbacklight 255 96 0\nkeys G1 G22 TOP\n");
        assert_eq!(s.stick, (0, 255));
        assert_eq!(s.backlight, [255, 96, 0]);
        assert_eq!(
            s.keys,
            ["G1", "G22", "TOP"].iter().map(|k| k.to_string()).collect()
        );
        let empty = State::parse("");
        assert_eq!((empty.stick, empty.backlight), ((128, 128), [0, 0, 255]));
        assert!(empty.keys.is_empty() && empty.lcd.is_none());
        assert!(State::parse("stick x y\nkeys\n").keys.is_empty());
    }

    #[test]
    fn built_in_asset_covers_every_control() {
        let (png, json) = generate();
        let asset = Asset::parse(&png, &json).unwrap();
        assert_eq!((asset.width, asset.height), (576.0, 816.0));
        let ids: HashSet<_> = asset.elements.iter().map(|e| e.id.as_str()).collect();
        for name in crate::profile::CONTROLS
            .iter()
            .filter(|n| !n.starts_with("STICK_"))
        {
            let id = if *name == "TOP" { "stick" } else { name }; // the click lights the cap
            assert!(ids.contains(id), "{name} has no sprite");
        }
        assert!(ids.contains("stick") && ids.contains("body") && ids.contains("LIGHT"));
        assert_eq!(asset.elements[0].id, "body");
        let lcd_box = asset.elements.iter().find(|e| e.id == "lcd").unwrap();
        let (lx, ly, lw, lh) = screen();
        assert!(
            lcd_box.pos[0] >= ((lx + 2) * UNIT) as f32
                && lcd_box.pos[0] + lcd::W as f32 <= ((lx + lw - 2) * UNIT) as f32
        );
        assert!(
            lcd_box.pos[1] >= ((ly + 2) * UNIT) as f32
                && lcd_box.pos[1] + lcd::H as f32 <= ((ly + lh - 2) * UNIT) as f32
        );
        for e in asset.elements.iter().filter(|e| e.id != "lcd") {
            let [sx, sy, w, h] = e.mapping;
            let bottom = if e.kind == 0 {
                sy + h
            } else {
                sy + 2.0 * h + PRESSED_GAP as f32
            };
            assert!(
                sx + w <= asset.image.width() as f32 && bottom <= asset.image.height() as f32,
                "{} off the sheet",
                e.id
            );
            assert!(
                e.pos[0] >= 0.0
                    && e.pos[1] >= 0.0
                    && e.pos[0] + w <= asset.width
                    && e.pos[1] + h <= asset.height,
                "{} off the overlay",
                e.id
            );
        }
    }

    /// The committed copies under assets/obs are what `--dump` writes; regenerate them
    /// (`g13map obs --dump assets/obs`) when the art changes.
    #[test]
    fn committed_asset_is_current() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/obs");
        let (png, json) = generate();
        assert_eq!(
            fs::read_to_string(dir.join(JSON)).unwrap(),
            json,
            "assets/obs/{JSON} is stale"
        );
        let committed = image::load_from_memory(&fs::read(dir.join(PNG)).unwrap())
            .unwrap()
            .into_rgba8();
        let fresh = image::load_from_memory(&png).unwrap().into_rgba8();
        assert!(
            committed.as_raw() == fresh.as_raw(),
            "assets/obs/{PNG} is stale"
        );
    }

    #[test]
    fn state_paths_follow_the_pipe() {
        assert!(state_path("keys").to_string_lossy().ends_with("_keys"));
        assert!(state_path("lcd").to_string_lossy().ends_with("_lcd"));
    }
}

// ---- the editor's OBS panel: the windows as toggles, the options beside them ----

/// The overlay processes found on this machine, by their command lines.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Running {
    pub overlay: Option<u32>,
    pub lcd: Option<u32>,
    pub calibrate: Option<u32>,
}

pub fn running() -> Running {
    let mut r = Running::default();
    let Ok(procs) = fs::read_dir("/proc") else {
        return r;
    };
    for entry in procs.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
        let Some(first) = args.first() else { continue };
        if !first.ends_with(b"g13map-obs") {
            continue;
        }
        if args.iter().any(|a| *a == b"--calibrate") {
            r.calibrate.get_or_insert(pid);
        } else if args.iter().any(|a| *a == b"--lcd") {
            r.lcd.get_or_insert(pid);
        } else {
            r.overlay.get_or_insert(pid);
        }
    }
    r
}

/// Starts `g13map-obs` beside this binary. The child outlives the editor (init reaps it
/// then); while the editor lives, the panel reaps it, or it lingers as a zombie.
pub fn launch(args: &[String]) -> Result<std::process::Child, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let overlay = exe.with_file_name("g13map-obs");
    std::process::Command::new(&overlay)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", overlay.display()))
}

pub fn stop(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status();
}

pub struct Panel {
    pub scale: u32,
    pub lcd_px: u32,
    pub chroma: bool,
    pub chroma_rgb: [u8; 3],
    status: String,
    running: Running,
    checked: Option<std::time::Instant>,
    children: Vec<std::process::Child>,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            scale: 1,
            lcd_px: UNIT as u32,
            chroma: false,
            chroma_rgb: [0, 255, 0],
            status: String::new(),
            running: Running::default(),
            checked: None,
            children: vec![],
        }
    }
}

impl Panel {
    fn refresh(&mut self) {
        if self
            .checked
            .is_none_or(|t| t.elapsed() > Duration::from_secs(1))
        {
            self.children
                .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
            self.running = running();
            self.checked = Some(std::time::Instant::now());
        }
    }

    fn start(&mut self, lcd: bool) {
        let mut args = vec![];
        if lcd {
            args.extend(["--lcd".to_string(), self.lcd_px.to_string()]);
        } else {
            args.extend(["--scale".to_string(), self.scale.to_string()]);
        }
        if self.chroma {
            let [r, g, b] = self.chroma_rgb;
            args.extend(["--background".to_string(), format!("{r:02x}{g:02x}{b:02x}")]);
        }
        self.status = match launch(&args) {
            Ok(child) => {
                self.children.push(child);
                String::new()
            }
            Err(e) => e,
        };
        self.checked = None;
    }

    /// The OBS window: the two overlay windows as toggles, their options, the sheet.
    pub fn window(&mut self, ctx: &egui::Context, open: &mut bool) {
        if !*open {
            return;
        }
        self.refresh();
        ctx.request_repaint_after(Duration::from_secs(1));
        let mut still_open = *open;
        egui::Window::new("OBS overlay")
            .open(&mut still_open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Windows for OBS to capture: the pad with its keys lit as pressed, the stick and the LCD; or the LCD alone. \
                         In OBS add a Window Capture (Xcomposite) and pick \"G13 overlay\" or \"G13 LCD\". \
                         The windows outlive the editor; keep them on a visible workspace.",
                    )
                    .weak(),
                );
                ui.add_space(6.0);
                egui::Grid::new("obs-grid").num_columns(3).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Pad overlay");
                    ui.add(egui::Slider::new(&mut self.scale, 1..=8).text("× scale"));
                    match self.running.overlay {
                        Some(pid) => {
                            if ui.button("Stop").clicked() {
                                stop(pid);
                                self.checked = None;
                            }
                        }
                        None => {
                            if ui.button("Open").clicked() {
                                self.start(false);
                            }
                        }
                    }
                    ui.end_row();
                    ui.label("LCD alone");
                    ui.add(egui::Slider::new(&mut self.lcd_px, 1..=16).text("px per LCD pixel"));
                    match self.running.lcd {
                        Some(pid) => {
                            if ui.button("Stop").clicked() {
                                stop(pid);
                                self.checked = None;
                            }
                        }
                        None => {
                            if ui.button("Open").clicked() {
                                self.start(true);
                            }
                        }
                    }
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.chroma, "Opaque background to chroma-key")
                        .on_hover_text("Instead of a transparent window (for a capture that drops alpha)");
                    ui.add_enabled_ui(self.chroma, |ui| {
                        ui.color_edit_button_srgb(&mut self.chroma_rgb);
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Write sheet for repainting")
                        .on_hover_text("g13.png + g13.json into ~/.config/g13map/obs; a repainted copy there is drawn instead of the built-in one")
                        .clicked()
                    {
                        self.status = match dump(&crate::config_dir().join("obs")) {
                            Ok(msg) => msg,
                            Err(e) => e,
                        };
                    }
                    match self.running.calibrate {
                        Some(pid) => {
                            if ui.button("Stop matching").clicked() {
                                stop(pid);
                                self.checked = None;
                            }
                        }
                        None => {
                            if ui
                                .button("Match the glass…")
                                .on_hover_text("A course of backlight colours to match by eye against the pad; writes ~/.config/g13map/glass, which the windows and the OBS source draw through")
                                .clicked()
                            {
                                self.status = match launch(&["--calibrate".to_string()]) {
                                    Ok(child) => {
                                        self.children.push(child);
                                        String::new()
                                    }
                                    Err(e) => e,
                                };
                                self.checked = None;
                            }
                        }
                    }
                });
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
        *open = still_open;
    }
}
