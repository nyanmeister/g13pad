// SPDX-License-Identifier: GPL-3.0-or-later
//! The LCD: 160x43 one-bit pixels in the daemon's byte layout, image conversion, and the
//! history of images under `~/.config/g13map/lcd/`.
//!
//! Layout (g13_lcd.cpp, pbm2lpbm.cpp): a byte is a column of eight pixels, byte
//! `x + (y / 8) * 160`, bit `y % 8`; 960 bytes cover 48 rows, the last five unused. A set bit
//! is a bright pixel on the backlit panel (verified on the hardware, 2026-09-29).
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

pub const W: usize = 160;
pub const H: usize = 43;
pub const BYTES: usize = 960;
/// The daemon's own start-up picture, from its source tree (`bitmaps/logo.lpbm`).
pub const LOGO: &[u8; BYTES] = include_bytes!("../assets/logo.lpbm");

#[derive(Clone, PartialEq, Debug)]
pub struct Bitmap(pub Vec<u8>);

impl Bitmap {
    pub fn blank() -> Self {
        Bitmap(vec![0; BYTES])
    }
    pub fn logo() -> Self {
        Bitmap(LOGO.to_vec())
    }
    pub fn from_lpbm(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != BYTES {
            return Err(format!("LCD image is {} bytes, not {BYTES}", bytes.len()));
        }
        Ok(Bitmap(bytes.to_vec()))
    }
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.0[x + (y / 8) * W] >> (y % 8) & 1 == 1
    }
    pub fn set(&mut self, x: usize, y: usize, on: bool) {
        let m = 1u8 << (y % 8);
        let b = &mut self.0[x + (y / 8) * W];
        if on {
            *b |= m;
        } else {
            *b &= !m;
        }
    }
    /// Grey levels for a preview, row-major 160x43: a set bit is bright on the panel.
    pub fn gray(&self) -> Vec<u8> {
        (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .map(|(x, y)| if self.get(x, y) { 215 } else { 28 })
            .collect()
    }
    #[cfg(test)]
    pub fn lit(&self) -> usize {
        (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .filter(|&(x, y)| self.get(x, y))
            .count()
    }
}

/// The slice of the source that goes on the panel, in source pixels. It may reach outside
/// the picture (filled with Options::background) and need not have the panel's shape.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg(feature = "editor")]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[cfg(feature = "editor")]
impl Crop {
    pub const ASPECT: f32 = W as f32 / H as f32;
    /// Smallest panel-shaped slice holding the whole picture, centred: letterboxed.
    pub fn fit(sw: u32, sh: u32) -> Crop {
        let (sw, sh) = (sw as f32, sh as f32);
        let (w, h) = if sw / sh >= Self::ASPECT {
            (sw, sw / Self::ASPECT)
        } else {
            (sh * Self::ASPECT, sh)
        };
        Crop {
            x: (sw - w) / 2.0,
            y: (sh - h) / 2.0,
            w,
            h,
        }
    }
    /// Largest panel-shaped slice inside the picture, centred: cropped.
    pub fn fill(sw: u32, sh: u32) -> Crop {
        let (sw, sh) = (sw as f32, sh as f32);
        let (w, h) = if sw / sh >= Self::ASPECT {
            (sh * Self::ASPECT, sh)
        } else {
            (sw, sw / Self::ASPECT)
        };
        Crop {
            x: (sw - w) / 2.0,
            y: (sh - h) / 2.0,
            w,
            h,
        }
    }
    /// The whole picture, stretched to the panel.
    pub fn whole(sw: u32, sh: u32) -> Crop {
        Crop {
            x: 0.0,
            y: 0.0,
            w: sw as f32,
            h: sh as f32,
        }
    }
    /// Whether it has the panel's shape (within a pixel's worth).
    pub fn panel_shaped(&self) -> bool {
        (self.w / self.h - Self::ASPECT).abs() * H as f32 <= 1.0
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg(feature = "editor")]
pub struct Options {
    /// None: the letterboxed whole picture (`Crop::fit`).
    pub crop: Option<Crop>,
    /// Grey below this lights the pixel; 128 is mid-grey, higher lights more pixels.
    pub level: u8,
    /// Share of the error Floyd–Steinberg carries on, 0 (a flat threshold) to 1 (all of it).
    pub dither: f32,
    pub invert: bool,
    /// Grey outside the picture and behind transparent pixels, before level/dither/invert.
    pub background: u8,
}

#[cfg(feature = "editor")]
impl Default for Options {
    fn default() -> Self {
        Options {
            crop: None,
            level: 128,
            dither: 1.0,
            invert: false,
            background: 255,
        }
    }
}

#[cfg(feature = "editor")]
impl Options {
    /// The kept form: one `key value` per line.
    pub fn to_text(self) -> String {
        let mut s = String::new();
        if let Some(c) = &self.crop {
            s.push_str(&format!("crop {} {} {} {}\n", c.x, c.y, c.w, c.h));
        }
        s.push_str(&format!(
            "level {}\ndither {}\ninvert {}\nbackground {}\n",
            self.level, self.dither, self.invert as u8, self.background
        ));
        s
    }
    pub fn parse(text: &str) -> Options {
        let mut o = Options::default();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let key = it.next().unwrap_or("");
            let Some(nums) = it
                .map(|v| v.parse::<f32>().ok().filter(|n| n.is_finite()))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            match (key, nums.as_slice()) {
                ("crop", [x, y, w, h]) if *w > 0.0 && *h > 0.0 => {
                    o.crop = Some(Crop {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                    })
                }
                ("level", [l]) => o.level = l.round().clamp(0.0, 255.0) as u8,
                ("dither", [d]) => o.dither = d.clamp(0.0, 1.0),
                ("invert", [i]) => o.invert = *i != 0.0,
                ("background", [b]) => o.background = b.round().clamp(0.0, 255.0) as u8,
                _ => {}
            }
        }
        o
    }
}

/// A greyed frame summed (an integral image) so a slice's pixels are exact box averages
/// whatever the scale. The interpolated integral of a piecewise-constant picture is the
/// bilinear interpolation of its integral image, so fractional slice edges cost nothing
/// extra.
#[cfg(feature = "editor")]
struct Plane {
    /// The summed copy may be smaller than the file (capped at four megapixels, more than
    /// the panel can use); `k` scales file pixels to it.
    sw: u32,
    sh: u32,
    k: f32,
    sum: Vec<u32>,
    transparent: Vec<u32>,
}

#[cfg(feature = "editor")]
impl Plane {
    /// Sums `small`, a copy of the frame at `k` times the file's size.
    fn new(small: &image::GrayAlphaImage, k: f32) -> Plane {
        let (sw, sh) = small.dimensions();
        // sum[(y) * (sw + 1) + x] is the total of the pixels in [0, x) × [0, y).
        let stride = sw as usize + 1;
        let mut sum = vec![0u32; stride * (sh as usize + 1)];
        let mut transparent = vec![0u32; sum.len()];
        for y in 0..sh as usize {
            let mut row = 0u32;
            let mut alpha_row = 0u32;
            for x in 0..sw as usize {
                row += small.get_pixel(x as u32, y as u32).0[0] as u32;
                alpha_row += small.get_pixel(x as u32, y as u32).0[1] as u32;
                sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
                transparent[(y + 1) * stride + x + 1] = transparent[y * stride + x + 1] + alpha_row;
            }
        }
        Plane {
            sw,
            sh,
            k,
            sum,
            transparent,
        }
    }
    /// The integral over [0, x) × [0, y), x and y in summed pixels, already inside the picture.
    fn integral(&self, x: f32, y: f32, sum: &[u32]) -> f64 {
        let stride = self.sw as usize + 1;
        let (xi, yi) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = ((x - xi as f32) as f64, (y - yi as f32) as f64);
        let (xi, yi) = (xi.min(self.sw as usize - 1), yi.min(self.sh as usize - 1));
        let at = |xx: usize, yy: usize| sum[yy * stride + xx] as f64;
        let (fx, fy) = if x.floor() as usize >= self.sw as usize {
            (1.0, fy)
        } else {
            (fx, fy)
        };
        let (fx, fy) = if y.floor() as usize >= self.sh as usize {
            (fx, 1.0)
        } else {
            (fx, fy)
        };
        (1.0 - fx) * (1.0 - fy) * at(xi, yi)
            + fx * (1.0 - fy) * at(xi + 1, yi)
            + (1.0 - fx) * fy * at(xi, yi + 1)
            + fx * fy * at(xi + 1, yi + 1)
    }
    /// Mean grey over a box in file pixels, composited over the chosen background.
    fn mean(&self, x0: f32, y0: f32, x1: f32, y1: f32, background: u8) -> f32 {
        let (x0, y0, x1, y1) = (x0 * self.k, y0 * self.k, x1 * self.k, y1 * self.k);
        let area = ((x1 - x0) * (y1 - y0)) as f64;
        if area <= 0.0 {
            return background as f32;
        }
        let (cx0, cy0) = (x0.clamp(0.0, self.sw as f32), y0.clamp(0.0, self.sh as f32));
        let (cx1, cy1) = (x1.clamp(0.0, self.sw as f32), y1.clamp(0.0, self.sh as f32));
        let inside = ((cx1 - cx0) * (cy1 - cy0)) as f64;
        if inside <= 0.0 {
            return background as f32;
        }
        let box_sum = |sum: &[u32]| {
            self.integral(cx1, cy1, sum)
                - self.integral(cx0, cy1, sum)
                - self.integral(cx1, cy0, sum)
                + self.integral(cx0, cy0, sum)
        };
        let white = box_sum(&self.sum);
        let transparency = box_sum(&self.transparent) / 255.0;
        ((white - (255.0 - background as f64) * transparency + background as f64 * (area - inside))
            / area) as f32
    }
    /// The panel's grey levels for a slice, row-major 160x43.
    fn sample(&self, crop: &Crop, background: u8) -> Vec<f32> {
        let (dx, dy) = (crop.w / W as f32, crop.h / H as f32);
        let mut buf = Vec::with_capacity(W * H);
        for y in 0..H {
            for x in 0..W {
                let (x0, y0) = (crop.x + x as f32 * dx, crop.y + y as f32 * dy);
                buf.push(self.mean(x0, y0, x0 + dx, y0 + dy, background));
            }
        }
        buf
    }
}

/// A decoded picture ready to render any slice of: composited over white, greyed and summed
/// (`Plane`); for an animation (GIF, WebP, APNG) every frame besides, greyed and kept at one
/// scale, with its delay.
#[cfg(feature = "editor")]
pub struct Source {
    pub path: PathBuf,
    /// Size in the file, the unit `Crop` uses.
    pub w: u32,
    pub h: u32,
    /// The first frame.
    plane: Plane,
    /// A copy small enough for the screen (longest side at most `VIEW`).
    view: image::GrayAlphaImage,
    /// All frames of an animation (one or none for a still), at `fk` times the file's size.
    frames: Vec<(Duration, image::GrayAlphaImage)>,
    fk: f32,
}

#[cfg(feature = "editor")]
const CAP: u32 = 4_000_000;
/// Grey and transparency bytes the frames may take together; past it they are all halved.
#[cfg(feature = "editor")]
const BUDGET: usize = 192 << 20;
#[cfg(feature = "editor")]
pub const VIEW: u32 = 900;

#[cfg(feature = "editor")]
fn grey(img: &image::RgbaImage) -> image::GrayAlphaImage {
    image::GrayAlphaImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let a = p[3] as f32 / 255.0;
        let l = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
        // First channel: grey composited over white. Second: transparency (not opacity).
        image::LumaA([(l * a + 255.0 * (1.0 - a)).round() as u8, 255 - p[3]])
    })
}

#[cfg(feature = "editor")]
fn shrink(g: &image::GrayAlphaImage, k: f32) -> image::GrayAlphaImage {
    let (w, h) = g.dimensions();
    let (nw, nh) = (
        ((w as f32 * k) as u32).max(1),
        ((h as f32 * k) as u32).max(1),
    );
    image::imageops::resize(g, nw, nh, image::imageops::FilterType::Triangle)
}

/// The scale that keeps `w`×`h` under `cap` pixels.
#[cfg(feature = "editor")]
fn cap_scale(w: u32, h: u32, cap: u32) -> f32 {
    let pixels = u64::from(w) * u64::from(h);
    if pixels > u64::from(cap) {
        (cap as f32 / pixels as f32).sqrt()
    } else {
        1.0
    }
}

/// A frame's delay as the file states it. Delays of 10 ms and under mean "as fast as you
/// can" in GIFs, which browsers show at 100 ms; the same here.
#[cfg(feature = "editor")]
fn frame_delay(f: &image::Frame) -> Duration {
    let (n, d) = f.delay().numer_denom_ms();
    let ms = if d == 0 {
        100.0
    } else {
        (n as f64 / d as f64).round()
    };
    Duration::from_millis(if ms <= 10.0 { 100 } else { ms as u64 })
}

/// Decodes the file: the first frame in colour, and for an animation every frame greyed at a
/// shared scale (halved as often as the budget asks) with its delay. A still has no frames.
/// A frame that fails to decode ends the animation there (a truncated GIF keeps what it
/// had); a first frame that fails is the file's error.
#[cfg(feature = "editor")]
type Decoded = (
    image::RgbaImage,
    Vec<(Duration, image::GrayAlphaImage)>,
    f32,
);

#[cfg(feature = "editor")]
fn decode(path: &Path) -> Result<Decoded, String> {
    use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
    use image::{AnimationDecoder, ImageFormat};
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let reader = image::ImageReader::open(path)
        .map_err(|e| err(&e))?
        .with_guessed_format()
        .map_err(|e| err(&e))?;
    let format = reader.format();
    let file = reader.into_inner();
    let frames = match format {
        Some(ImageFormat::Gif) => Some(GifDecoder::new(file).map_err(|e| err(&e))?.into_frames()),
        Some(ImageFormat::WebP) => {
            let d = WebPDecoder::new(file).map_err(|e| err(&e))?;
            d.has_animation().then(|| d.into_frames())
        }
        Some(ImageFormat::Png) => {
            let d = PngDecoder::new(file).map_err(|e| err(&e))?;
            if d.is_apng().map_err(|e| err(&e))? {
                Some(d.apng().map_err(|e| err(&e))?.into_frames())
            } else {
                None
            }
        }
        _ => None,
    };
    let Some(frames) = frames else {
        let img = image::open(path).map_err(|e| err(&e))?.to_rgba8();
        return Ok((img, vec![], 1.0));
    };
    let mut first: Option<image::RgbaImage> = None;
    let mut out: Vec<(Duration, image::GrayAlphaImage)> = vec![];
    let mut fk = 1.0f32;
    let mut bytes = 0usize;
    for f in frames {
        let f = match f {
            Ok(f) => f,
            Err(e) if first.is_none() => return Err(err(&e)),
            Err(_) => break,
        };
        let delay = frame_delay(&f);
        let rgba = f.into_buffer();
        if first.is_none() {
            fk = cap_scale(rgba.width(), rgba.height(), CAP);
            first = Some(rgba.clone());
        }
        let g = grey(&rgba);
        let small = if fk < 1.0 { shrink(&g, fk) } else { g };
        bytes += small.len();
        out.push((delay, small));
        while bytes > BUDGET {
            fk /= 2.0;
            bytes = 0;
            for (_, g) in &mut out {
                *g = shrink(g, 0.5);
                bytes += g.len();
            }
        }
    }
    match first {
        Some(img) if out.len() > 1 => Ok((img, out, fk)),
        Some(img) => Ok((img, vec![], 1.0)),
        None => Err(err(&"no frames")),
    }
}

#[cfg(feature = "editor")]
impl Source {
    #[cfg(test)]
    pub fn fuzz_fixture() -> Self {
        let img = image::RgbaImage::from_fn(32, 16, |x, y| {
            image::Rgba([(x * 8) as u8, (y * 16) as u8, 127, ((x + y) * 5) as u8])
        });
        let g = grey(&img);
        Self {
            path: PathBuf::new(),
            w: 32,
            h: 16,
            plane: Plane::new(&g, 1.0),
            view: g,
            frames: vec![],
            fk: 1.0,
        }
    }
    /// The source selector, composited over the chosen background.
    pub fn view(&self, background: u8) -> image::GrayImage {
        image::GrayImage::from_fn(self.view.width(), self.view.height(), |x, y| {
            let [white, transparent] = self.view.get_pixel(x, y).0;
            image::Luma([
                (white as f32 - (255 - background) as f32 * transparent as f32 / 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8,
            ])
        })
    }
    pub fn open(path: &Path) -> Result<Source, String> {
        let (img, frames, fk) = decode(path)?;
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Err(format!("{}: empty image", path.display()));
        }
        let gray = grey(&img);
        let k = cap_scale(w, h, CAP);
        let small = if k < 1.0 {
            shrink(&gray, k)
        } else {
            gray.clone()
        };
        let k = small.width() as f32 / w as f32;
        let vk = VIEW as f32 / w.max(h) as f32;
        let view = if vk < 1.0 { shrink(&gray, vk) } else { gray };
        Ok(Source {
            path: path.to_path_buf(),
            w,
            h,
            plane: Plane::new(&small, k),
            view,
            frames,
            fk,
        })
    }
    /// More than one frame.
    pub fn animated(&self) -> bool {
        self.frames.len() > 1
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len().max(1)
    }
    /// The whole animation's length.
    pub fn duration(&self) -> Duration {
        self.frames.iter().map(|(d, _)| *d).sum()
    }
    #[cfg(test)]
    fn mean(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
        self.plane.mean(x0, y0, x1, y1, 255)
    }
    /// Renders the slice of the first frame for the panel: sampled, then Floyd–Steinberg
    /// dithered with `dither` of the error carried on (0 is a flat threshold at `level`).
    /// Dark lights up; `invert` swaps that.
    pub fn render(&self, o: &Options) -> Bitmap {
        let crop = o.crop.unwrap_or_else(|| Crop::fit(self.w, self.h));
        dither(self.plane.sample(&crop, o.background), o)
    }
    /// Every frame rendered the same way: one for a still.
    pub fn render_all(&self, o: &Options) -> Animation {
        if !self.animated() {
            return Animation::still(self.render(o));
        }
        let crop = o.crop.unwrap_or_else(|| Crop::fit(self.w, self.h));
        let frames = self
            .frames
            .iter()
            .map(|(d, g)| {
                (
                    *d,
                    dither(Plane::new(g, self.fk).sample(&crop, o.background), o),
                )
            })
            .collect();
        Animation { frames }
    }
}

/// Floyd–Steinberg over sampled grey levels (see `Source::render`).
#[cfg(feature = "editor")]
fn dither(mut buf: Vec<f32>, o: &Options) -> Bitmap {
    let level = o.level as f32;
    let mut bm = Bitmap::blank();
    for y in 0..H {
        for x in 0..W {
            let i = y * W + x;
            let old = buf[i];
            let dark = old < level;
            if o.dither > 0.0 {
                let err = (old - if dark { 0.0 } else { 255.0 }) * o.dither;
                if x + 1 < W {
                    buf[i + 1] += err * 7.0 / 16.0;
                }
                if y + 1 < H {
                    if x > 0 {
                        buf[i + W - 1] += err * 3.0 / 16.0;
                    }
                    buf[i + W] += err * 5.0 / 16.0;
                    if x + 1 < W {
                        buf[i + W + 1] += err / 16.0;
                    }
                }
            }
            bm.set(x, y, dark != o.invert);
        }
    }
    bm
}

// ---- animation: the frames as the panel shows them, each with its delay ----

/// Rendered frames with their delays; a still is one frame.
#[derive(Clone, PartialEq, Debug)]
pub struct Animation {
    pub frames: Vec<(Duration, Bitmap)>,
}

/// One record of a kept `.anim` file: the delay in ms (u32, little-endian) and the frame.
const RECORD: usize = 4 + BYTES;

impl Animation {
    pub fn still(bm: Bitmap) -> Animation {
        Animation {
            frames: vec![(Duration::ZERO, bm)],
        }
    }
    pub fn first(&self) -> &Bitmap {
        &self.frames[0].1
    }
    pub fn animated(&self) -> bool {
        self.frames.len() > 1
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.frames.len() * RECORD);
        for (d, bm) in &self.frames {
            v.extend_from_slice(&(d.as_millis().min(u32::MAX as u128) as u32).to_le_bytes());
            v.extend_from_slice(&bm.0);
        }
        v
    }
    pub fn from_bytes(b: &[u8]) -> Result<Animation, String> {
        if b.is_empty() || !b.len().is_multiple_of(RECORD) {
            return Err(format!(
                "animation file is {} bytes, not a multiple of {RECORD}",
                b.len()
            ));
        }
        let frames = b
            .chunks(RECORD)
            .map(|r| {
                let ms = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
                (Duration::from_millis(ms as u64), Bitmap(r[4..].to_vec()))
            })
            .collect();
        Ok(Animation { frames })
    }
    /// The frame due at `t` into the loop and how long until the next one is due.
    pub fn due(&self, t: Duration) -> (usize, Duration) {
        let total: Duration = self
            .frames
            .iter()
            .map(|(d, _)| (*d).max(Duration::from_millis(1)))
            .sum();
        let mut at = Duration::from_nanos((t.as_nanos() % total.as_nanos()) as u64);
        for (i, (d, _)) in self.frames.iter().enumerate() {
            let d = (*d).max(Duration::from_millis(1));
            if at < d {
                return (i, d - at);
            }
            at -= d;
        }
        (0, Duration::ZERO)
    }
    /// Sends frames on the animation's clock until `stop`. The custom driver services the
    /// LCD independently of input; stock is slower when idle. Late frames are skipped.
    /// With `yield_to_editor` the panel is left alone while an editor is open.
    fn play(&self, stop: &AtomicBool, yield_to_editor: bool) {
        let t0 = Instant::now();
        let mut shown = Some(0usize);
        while !stop.load(Ordering::Relaxed) {
            if yield_to_editor && editor_open() {
                shown = None;
                thread::sleep(Duration::from_millis(250));
                continue;
            }
            let (i, next) = self.due(t0.elapsed());
            if shown != Some(i) {
                if crate::daemon::send_lcd(&self.frames[i].1 .0).is_err() {
                    // The daemon is gone or not reading; try again in a while.
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                shown = Some(i);
            }
            thread::sleep(next.clamp(Duration::from_millis(5), Duration::from_millis(50)));
        }
    }
}

/// A running animation on the panel; dropping it stops the frames.
pub struct Player {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Puts a kept picture (or, with no name, the daemon's logo) on the panel: the first frame
/// now, and for an animation the rest from a thread until the `Player` is dropped. The
/// watcher passes `yield_to_editor`; the editor, which holds `editor_lock`, does not.
pub fn show(name: Option<&str>, yield_to_editor: bool) -> Result<Player, String> {
    let first = match name {
        Some(n) => load(n)?,
        None => Bitmap::logo(),
    };
    // The watcher yields its first frame as well as subsequent animation frames. Otherwise
    // a profile switch can put one watcher frame between the editor's old/new animations.
    if !yield_to_editor || !editor_open() {
        crate::daemon::send_lcd(&first.0)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread = name
        .and_then(load_anim)
        .filter(|a| a.animated())
        .map(|anim| {
            let s = stop.clone();
            thread::spawn(move || anim.play(&s, yield_to_editor))
        });
    Ok(Player { stop, thread })
}

/// Taken by the editor for its life: while it is held, the watcher's animation leaves the
/// panel to the editor. Dropping the file releases it.
pub fn editor_lock() -> Option<fs::File> {
    let p = crate::daemon::runtime_file("editor").ok()?;
    let f = fs::File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&p)
        .ok()?;
    f.lock().ok()?;
    Some(f)
}

/// Whether an editor holds the lock.
pub fn editor_open() -> bool {
    let Ok(f) = crate::daemon::runtime_file("editor").and_then(|p| {
        fs::File::options()
            .write(true)
            .open(p)
            .map_err(|e| e.to_string())
    }) else {
        return false;
    };
    match f.try_lock() {
        Ok(()) => {
            let _ = f.unlock();
            false
        }
        Err(fs::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

/// Decodes an image file and renders it for the panel with `o`.
#[cfg(test)]
#[cfg(feature = "editor")]
pub fn convert(path: &Path, o: &Options) -> Result<Bitmap, String> {
    Ok(Source::open(path)?.render(o))
}

// ---- history: NAME.lpbm (the 960 bytes), NAME.orig.EXT (a copy of the source) and NAME.conv
// (the options that made it) ----

pub fn dir() -> PathBuf {
    crate::config_dir().join("lcd")
}

pub fn path(name: &str) -> PathBuf {
    dir().join(format!("{name}.lpbm"))
}

/// The original behind a kept image, if its copy is there.
pub fn source(name: &str) -> Option<PathBuf> {
    let prefix = format!("{name}.orig.");
    fs::read_dir(dir())
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.starts_with(&prefix))
        })
}

/// What a kept name was made from, when it is a user's own: a converted picture (a copy of
/// the file beside it) or kept text. A built-in scene must not overwrite either.
pub fn origin(name: &str) -> Option<&'static str> {
    if source(name).is_some() {
        Some("a picture converted from a file")
    } else if dir().join(format!("{name}.text")).is_file() {
        Some("kept text")
    } else {
        None
    }
}

/// The options a kept image was made with, if they were kept (images from before the
/// adjust window have none: they were `Fit`, dithered).
#[cfg(feature = "editor")]
pub fn options(name: &str) -> Option<Options> {
    fs::read_to_string(dir().join(format!("{name}.conv")))
        .ok()
        .map(|t| Options::parse(&t))
}

/// Kept images, most recently used first.
pub fn names() -> Vec<String> {
    let mut v: Vec<(SystemTime, String)> = fs::read_dir(dir())
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension()? != "lpbm" {
                        return None;
                    }
                    let m = e.metadata().ok()?.modified().ok()?;
                    Some((m, p.file_stem()?.to_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    v.into_iter().map(|(_, n)| n).collect()
}

pub fn load(name: &str) -> Result<Bitmap, String> {
    let p = path(name);
    fs::read(&p)
        .map_err(|e| format!("{}: {e}", p.display()))
        .and_then(|b| Bitmap::from_lpbm(&b))
}

/// Marks an image as used now, so `names` lists it first.
pub fn touch(name: &str) {
    if let Ok(f) = fs::File::options().write(true).open(path(name)) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Profiles whose picture this is.
pub fn users(name: &str) -> Vec<String> {
    crate::profile_names()
        .into_iter()
        .filter(|p| crate::load(p).is_ok_and(|(prof, _)| prof.lcd.as_deref() == Some(name)))
        .collect()
}

fn stem_of(source: &Path) -> String {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    // A kept copy loaded back (`NAME.orig.jpg`) is NAME again, not "NAMEorig".
    let stem = stem.strip_suffix(".orig").unwrap_or(stem);
    let stem: String = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_".contains(*c))
        .take(32) // hash-named files would make unreadable, panel-wide names
        .collect();
    if stem.trim().is_empty() {
        "image".to_string()
    } else {
        stem.trim().to_string()
    }
}

/// The history name for a source file: the entry already made from the same bytes, else the
/// file's stem, numbered if another file took it.
pub fn name_for(source: &Path) -> Result<String, String> {
    let stem = stem_of(source);
    let bytes = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    let mut name = stem.clone();
    let mut n = 1;
    loop {
        match self::source(&name) {
            Some(orig) if fs::read(&orig).ok().as_deref() == Some(&bytes[..]) => return Ok(name),
            None if !path(&name).exists() => return Ok(name),
            _ => {
                n += 1;
                name = format!("{stem}-{n}");
            }
        }
    }
}

/// A free name beside an existing one, for a second slice of the same picture.
pub fn fork_name(name: &str) -> String {
    let stem = match name.rsplit_once('-') {
        Some((s, n)) if n.parse::<u32>().is_ok() && !s.is_empty() => s,
        _ => name,
    };
    let mut n = 2;
    loop {
        let candidate = format!("{stem}-{n}");
        if !path(&candidate).exists() && source(&candidate).is_none() {
            return candidate;
        }
        n += 1;
    }
}

fn anim_path(name: &str) -> PathBuf {
    dir().join(format!("{name}.anim"))
}

/// The kept animation behind a name, if it has more than one frame (`NAME.anim`).
pub fn load_anim(name: &str) -> Option<Animation> {
    let b = fs::read(anim_path(name)).ok()?;
    Animation::from_bytes(&b).ok().filter(|a| a.animated())
}

/// How many frames a kept picture has: one for a still.
pub fn frame_count(name: &str) -> usize {
    fs::metadata(anim_path(name))
        .map(|m| (m.len() as usize / RECORD).max(1))
        .unwrap_or(1)
}

/// Keeps a converted image under `name` with a copy of the source beside it (so it can be
/// converted again) and the options that made it. The first frame is `NAME.lpbm`; an
/// animation's frames go to `NAME.anim` besides (a still removes a stale one).
#[cfg(feature = "editor")]
pub fn keep(name: &str, source: &Path, anim: &Animation, o: &Options) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| format!("{}: {e}", dir().display()))?;
    if self::source(name).is_none() {
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("img")
            .to_ascii_lowercase();
        let orig = dir().join(format!("{name}.orig.{ext}"));
        fs::copy(source, &orig).map_err(|e| format!("{}: {e}", orig.display()))?;
    }
    let conv = dir().join(format!("{name}.conv"));
    fs::write(&conv, o.to_text()).map_err(|e| format!("{}: {e}", conv.display()))?;
    keep_animation(name, anim)
}

/// Saves panel frames without requiring an image source (also used for generated text).
pub fn keep_animation(name: &str, anim: &Animation) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| format!("{}: {e}", dir().display()))?;
    let a = anim_path(name);
    if anim.animated() {
        let tmp = a.with_extension("anim.new");
        fs::write(&tmp, anim.to_bytes())
            .and_then(|_| fs::rename(&tmp, &a))
            .map_err(|e| format!("{}: {e}", a.display()))?;
    } else if a.exists() {
        fs::remove_file(&a).map_err(|e| format!("{}: {e}", a.display()))?;
    }
    let p = path(name);
    let tmp = p.with_extension("lpbm.new");
    fs::write(&tmp, &anim.first().0)
        .and_then(|_| fs::rename(&tmp, &p))
        .map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(all(test, feature = "editor"))]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_daemon() {
        // pbm2lpbm: pixel (x, y) is byte x + (y / 8) * 160, bit y % 8.
        let mut b = Bitmap::blank();
        b.set(3, 10, true);
        let preview = b.gray();
        assert!(preview[10 * W + 3] > preview[10 * W + 4]);
        assert_eq!(b.0[3 + 160], 1 << 2);
        assert!(b.get(3, 10) && !b.get(3, 11) && !b.get(4, 10));
        b.set(3, 10, false);
        assert_eq!(b.lit(), 0);
        assert_eq!(Bitmap::logo().0.len(), BYTES);
        assert!(Bitmap::logo().lit() > 100);
        assert!(Bitmap::from_lpbm(&[0; 959]).is_err());
    }

    #[test]
    fn background_covers_margins_transparency_and_animation() {
        let sandbox = crate::test_support::Sandbox::new("background");
        let p = sandbox.dir.join("alpha.png");
        image::RgbaImage::from_fn(160, 43, |x, _| match x {
            0..=39 => image::Rgba([0, 0, 0, 255]),
            40..=79 => image::Rgba([255, 255, 255, 255]),
            80..=119 => image::Rgba([200, 20, 40, 0]),
            _ => image::Rgba([255, 255, 255, 128]),
        })
        .save(&p)
        .unwrap();
        let mut src = Source::open(&p).unwrap();
        let opts = Options {
            crop: Some(Crop::whole(160, 43)),
            dither: 0.0,
            background: 0,
            ..Default::default()
        };
        let black = src.render(&opts);
        let white = src.render(&Options {
            background: 255,
            ..opts
        });
        for y in 0..H {
            assert!(black.get(0, y) && white.get(0, y));
            assert!(!black.get(50, y) && !white.get(50, y));
            assert!(black.get(90, y) && !white.get(90, y));
            assert!(!black.get(130, y) && !white.get(130, y));
        }
        let view = src.view(73);
        assert_eq!(view.get_pixel(90, 21).0[0], 73);
        assert!((163..=165).contains(&view.get_pixel(130, 21).0[0]));
        let outside = Options {
            crop: Some(Crop {
                x: -200.0,
                y: 0.0,
                w: 160.0,
                h: 43.0,
            }),
            ..opts
        };
        assert_eq!(src.render(&outside).lit(), W * H);
        assert_eq!(
            src.render(&Options {
                background: 255,
                ..outside
            })
            .lit(),
            0
        );
        // Exact box average across an opaque/transparent edge and the image boundary.
        assert!((src.plane.mean(79.5, 0.0, 80.5, 1.0, 73) - 164.0).abs() < 0.01);
        assert!((src.plane.mean(-0.5, 0.0, 0.5, 1.0, 73) - 36.5).abs() < 0.01);
        let frame = grey(&image::open(&p).unwrap().to_rgba8());
        src.frames = vec![
            (Duration::from_millis(50), frame.clone()),
            (Duration::from_millis(80), frame),
        ];
        let animation = src.render_all(&opts);
        assert!(animation.frames.iter().all(|(_, bm)| bm == &black));
        assert_eq!(animation.frames[1].0, Duration::from_millis(80));
        assert_eq!(Options::parse("level 128\n").background, 255);
    }

    /// A 200x100 picture, black on the left half: box means are exact at any scale.
    /// `tag` keeps parallel tests out of each other's files: two tests writing one PNG at
    /// once let the other decode a half-written file (flaked once, 2026-09-29).
    fn halves(tag: &str) -> (Source, PathBuf) {
        let dir = std::env::temp_dir().join(format!("g13map-src-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("halves.png");
        image::GrayImage::from_fn(200, 100, |x, _| {
            image::Luma([if x < 100 { 0 } else { 255 }])
        })
        .save(&p)
        .unwrap();
        (Source::open(&p).unwrap(), p)
    }

    #[test]
    fn samples_exact_box_means() {
        let (s, _) = halves("box");
        assert_eq!(s.mean(0.0, 0.0, 100.0, 100.0), 0.0);
        assert_eq!(s.mean(100.0, 0.0, 200.0, 100.0), 255.0);
        assert!((s.mean(0.0, 0.0, 200.0, 100.0) - 127.5).abs() < 0.01);
        // Fractional edges: a box from 99.5 to 100.5 is half black.
        assert!((s.mean(99.5, 10.0, 100.5, 20.0) - 127.5).abs() < 0.01);
        // Outside the picture is white: a box half off the left edge over black is mid-grey.
        assert!((s.mean(-50.0, 0.0, 50.0, 100.0) - 127.5).abs() < 0.01);
        // Fill on a 2:1 picture takes the middle band; Whole stretches; Fit (the picture is
        // taller than the panel's 3.7:1) pads the sides: the picture spans panel columns 37
        // to 123, black up to 80.
        let fill = s.render(&Options {
            crop: Some(Crop::fill(200, 100)),
            ..Default::default()
        });
        let whole = s.render(&Options {
            crop: Some(Crop::whole(200, 100)),
            ..Default::default()
        });
        for bm in [&fill, &whole] {
            assert!(bm.get(0, 0) && bm.get(79, 42) && !bm.get(80, 0) && !bm.get(159, 42));
            assert_eq!(bm.lit(), W * H / 2);
        }
        let fit = s.render(&Options::default());
        assert!(!fit.get(0, 21) && !fit.get(159, 21) && fit.get(50, 21) && !fit.get(100, 21));
        assert!(fit.lit() < W * H / 2);
        let inverted = s.render(&Options {
            invert: true,
            ..Default::default()
        });
        assert_eq!(inverted.lit(), W * H - fit.lit());
    }

    #[test]
    fn level_and_dither_do_what_they_say() {
        let (s, _) = halves("level");
        let grey = |level, dither| {
            // A slice straddling the edge at eight source pixels per panel pixel: every panel
            // column is a uniform grey between the halves only in the one crossing column.
            let crop = Crop {
                x: 100.0 - 80.0 * 0.5,
                y: 0.0,
                w: 80.0,
                h: 21.5,
            };
            let o = Options {
                crop: Some(crop),
                level,
                dither,
                invert: false,
                background: 255,
            };
            s.render(&o)
        };
        // Flat threshold: mid-grey nowhere, so the split is at the picture's edge.
        let flat = grey(128, 0.0);
        assert_eq!(flat.lit(), W * H / 2);
        // A crop entirely on the black side lights everything at any level above 0, and
        // nothing at level 0.
        let black = Options {
            crop: Some(Crop {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 21.5,
            }),
            level: 1,
            dither: 0.0,
            invert: false,
            background: 255,
        };
        assert_eq!(s.render(&black).lit(), W * H);
        assert_eq!(s.render(&Options { level: 0, ..black }).lit(), 0);
        // A uniform grey of 127: a flat cut at level 128 lights all of it, at 127 none;
        // dithering lights about half, whatever the level near the middle.
        let dir = std::env::temp_dir().join(format!("g13map-grey-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("grey.png");
        image::GrayImage::from_pixel(50, 50, image::Luma([127]))
            .save(&p)
            .unwrap();
        let g = Source::open(&p).unwrap();
        let mid = |level, dither| Options {
            crop: Some(Crop::fill(50, 50)),
            level,
            dither,
            invert: false,
            background: 255,
        };
        assert_eq!(g.render(&mid(128, 0.0)).lit(), W * H);
        assert_eq!(g.render(&mid(127, 0.0)).lit(), 0);
        let share = g.render(&mid(128, 1.0)).lit() as f32 / (W * H) as f32;
        assert!((0.45..0.55).contains(&share), "{share}");
        // Half the error carried on still averages out to about half.
        let share = g.render(&mid(128, 0.5)).lit() as f32 / (W * H) as f32;
        assert!((0.3..0.7).contains(&share), "{share}");
        let _ = fs::remove_dir_all(&dir); // another test may have just put the writer lock here
    }

    #[test]
    fn crops_and_options_round_trip() {
        assert_eq!(
            Options::parse("crop NaN inf 160 43\ndither NaN\nlevel inf\n"),
            Options::default()
        );
        let scale = cap_scale(u32::MAX, u32::MAX, CAP);
        assert!(scale.is_finite() && scale > 0.0 && scale < 1.0);
        assert!(Crop::fit(200, 100).panel_shaped() && Crop::fill(200, 100).panel_shaped());
        assert!(!Crop::whole(200, 100).panel_shaped());
        let fit = Crop::fit(100, 100);
        assert!((fit.w - 100.0 * Crop::ASPECT).abs() < 0.01 && fit.h == 100.0 && fit.x < 0.0);
        let fill = Crop::fill(100, 100);
        assert!(fill.w == 100.0 && fill.y > 0.0 && (fill.h - 100.0 / Crop::ASPECT).abs() < 0.01);
        let o = Options {
            crop: Some(Crop {
                x: 1.5,
                y: -2.0,
                w: 320.0,
                h: 86.0,
            }),
            level: 100,
            dither: 0.25,
            invert: true,
            background: 73,
        };
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("junk\ncrop 1 2 0 4\n"), Options::default());
    }

    #[test]
    fn converts_a_portrait_fixture() {
        let _sandbox = crate::test_support::Sandbox::new("portrait");
        let dir = crate::config_dir();
        fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("portrait.png");
        image::GrayImage::from_fn(598, 896, |x, y| {
            image::Luma([if (149..449).contains(&x) && (200..696).contains(&y) {
                0
            } else {
                255
            }])
        })
        .save(&photo)
        .unwrap();
        let s = Source::open(&photo).unwrap();
        // Portrait input: Fit leaves the sides blank, Fill does not.
        let fit = s.render(&Options::default());
        let fill = s.render(&Options {
            crop: Some(Crop::fill(s.w, s.h)),
            ..Default::default()
        });
        let left_col = |b: &Bitmap| (0..H).filter(|&y| b.get(0, y)).count();
        assert_eq!(left_col(&fit), 0);
        assert!(fit.lit() > 0 && fill.lit() > 0);
        assert!(s.view.width() <= VIEW && s.view.height() <= VIEW);
    }

    #[test]
    fn history_keeps_and_names() {
        let _sandbox = crate::test_support::Sandbox::new("history");
        let dir = crate::config_dir();
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("pic.png");
        let b = dir.join("pic2").join("pic.png");
        fs::create_dir_all(b.parent().unwrap()).unwrap();
        image::GrayImage::from_pixel(4, 4, image::Luma([0]))
            .save(&a)
            .unwrap();
        image::GrayImage::from_pixel(4, 4, image::Luma([255]))
            .save(&b)
            .unwrap();
        let o = Options {
            level: 200,
            ..Default::default()
        };
        let bm_a = convert(&a, &o).unwrap();
        let bm_b = convert(&b, &o).unwrap();
        let long = dir.join(format!("{}.png", "x".repeat(70)));
        fs::copy(&a, &long).unwrap();
        assert_eq!(name_for(&long).unwrap(), "x".repeat(32)); // same bytes as pic, but pic is not kept yet
        assert_eq!(name_for(&a).unwrap(), "pic");
        keep("pic", &a, &Animation::still(bm_a.clone()), &o).unwrap();
        assert_eq!(name_for(&a).unwrap(), "pic"); // same source: same name
        assert_eq!(name_for(&b).unwrap(), "pic-2"); // other source, same stem
        keep("pic-2", &b, &Animation::still(bm_b.clone()), &o).unwrap();
        assert_eq!(load("pic").unwrap(), bm_a);
        assert_eq!(load("pic-2").unwrap(), bm_b);
        assert_eq!(options("pic").unwrap(), o);
        let orig = source("pic-2").unwrap();
        assert!(orig.ends_with("pic-2.orig.png"));
        assert_eq!(name_for(&orig).unwrap(), "pic-2"); // the kept copy loaded back
        assert_eq!(fork_name("pic"), "pic-3");
        assert_eq!(fork_name("pic-2"), "pic-3");
        touch("pic");
        assert_eq!(names()[0], "pic");
        fs::write(dir.join("profiles").join("x.bind"), "# lcd pic-2\n").ok();
        fs::create_dir_all(dir.join("profiles")).unwrap();
        fs::write(dir.join("profiles").join("x.bind"), "# lcd pic-2\n").unwrap();
        assert_eq!(users("pic-2"), vec!["x".to_string()]);
        assert!(users("pic").is_empty());
        let _ = fs::remove_dir_all(&dir); // another test may have just put the writer lock here
    }

    /// A three-frame GIF (left half black, right half black, all white; 100, 40 and 5 ms):
    /// every frame is decoded, greyed and rendered like a still; a 5 ms delay reads as 100.
    #[test]
    fn animations_decode_render_and_keep() {
        use image::codecs::gif::GifEncoder;
        let dir = std::env::temp_dir().join(format!("g13map-anim-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("blink.gif");
        let frame = |f: fn(u32) -> u8| {
            image::RgbaImage::from_fn(200, 100, |x, _| image::Rgba([f(x), f(x), f(x), 255]))
        };
        let frames = vec![
            image::Frame::from_parts(
                frame(|x| if x < 100 { 0 } else { 255 }),
                0,
                0,
                image::Delay::from_numer_denom_ms(100, 1),
            ),
            image::Frame::from_parts(
                frame(|x| if x < 100 { 255 } else { 0 }),
                0,
                0,
                image::Delay::from_numer_denom_ms(40, 1),
            ),
            image::Frame::from_parts(
                frame(|_| 255),
                0,
                0,
                image::Delay::from_numer_denom_ms(5, 1),
            ),
        ];
        let mut enc = GifEncoder::new(fs::File::create(&p).unwrap());
        enc.encode_frames(frames).unwrap();
        drop(enc);
        let s = Source::open(&p).unwrap();
        assert!(s.animated() && s.frame_count() == 3);
        assert_eq!(s.duration(), Duration::from_millis(240));
        let o = Options {
            crop: Some(Crop::fill(200, 100)),
            dither: 0.0,
            ..Default::default()
        };
        let a = s.render_all(&o);
        assert_eq!(a.frames.len(), 3);
        assert_eq!(a.frames[0].0, Duration::from_millis(100));
        assert_eq!(a.frames[1].0, Duration::from_millis(40));
        assert_eq!(a.frames[2].0, Duration::from_millis(100));
        assert_eq!(a.frames[0].1, s.render(&o));
        assert!(a.frames[0].1.get(0, 0) && !a.frames[0].1.get(159, 0));
        assert!(!a.frames[1].1.get(0, 0) && a.frames[1].1.get(159, 0));
        assert_eq!(a.frames[2].1.lit(), 0);
        assert_eq!(Animation::from_bytes(&a.to_bytes()).unwrap(), a);
        assert!(Animation::from_bytes(&[0u8; 10]).is_err());
        // The frame due at a time into the loop, and the wait for the next.
        assert_eq!(
            a.due(Duration::from_millis(0)),
            (0, Duration::from_millis(100))
        );
        assert_eq!(
            a.due(Duration::from_millis(130)),
            (1, Duration::from_millis(10))
        );
        assert_eq!(
            a.due(Duration::from_millis(250)),
            (0, Duration::from_millis(90))
        );
        // A still through the same door is one frame.
        let (still, _) = halves("anim");
        assert!(!still.animated());
        assert_eq!(still.render_all(&o).frames.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
