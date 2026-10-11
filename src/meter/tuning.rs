// SPDX-License-Identifier: GPL-3.0-or-later
//! Tuning responsibilities for Health Mode.
use super::*;

/// A backlight band: the health it starts at and its colour. The tuning file lists them
/// (`band NAME FROM R G B`, asked 2026-10-08: the edges, the colours and extra bands in
/// the file, so a Doom health past 200 can have a colour of its own). The band at 0 is
/// death (lights out, flatline); the lowest above it is the alarm, whose beats flash
/// and whose colour the search for a pulse runs under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Band {
    pub name: String,
    pub from: u32,
    pub rgb: [u8; 3],
}

impl Band {
    pub(super) fn new(name: &str, from: u32, rgb: [u8; 3]) -> Band {
        Band {
            name: name.into(),
            from,
            rgb,
        }
    }
    /// The G13's LED makes yellow and orange from red plus a little green; full green
    /// in the mix reads lime.
    pub(super) fn defaults() -> Vec<Band> {
        vec![
            Band::new("dead", 0, [0, 0, 0]),
            Band::new("red", 1, [255, 0, 0]),
            Band::new("orange", 26, [255, 48, 0]),
            Band::new("yellow", 51, [255, 215, 0]),
            Band::new("green", 76, [0, 255, 0]),
            Band::new("blue", 101, [0, 64, 255]),
        ]
    }
}

/// What the alarm band's beat flashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flash {
    Off,
    /// The trace and the heart, above the bars.
    Trace,
    Panel,
}

/// The meter's look: `~/.config/g13map/meter`, one `key values` per line, re-read by
/// the watcher within two seconds of a change, so a colour or a timing can be tried
/// on the glass without a rebuild (asked 2026-10-07). Absent keys keep their defaults;
/// a line that does not parse is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleStyle {
    Tread,
    Gear,
    Scan,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    /// The backlight bands, lowest first; never empty, the first starts at 0.
    pub bands: Vec<Band>,
    /// Seconds the dark flatline holds after a drop to zero.
    pub hold: f32,
    /// Seconds from beat to beat at full health, and on the last point.
    pub calm: f32,
    pub racing: f32,
    /// Seconds the heart swells on each beat (asked 2026-10-08: a file setting, so the
    /// look is tuned on the glass without a rebuild).
    pub swell: f32,
    /// What the alarm band's beats flash.
    pub flash: Flash,
    /// The heart's top-left corner, and the readout's right edge and top (pixels; the
    /// panel is 160 by 43).
    pub heart: [i32; 2],
    pub readout: [i32; 2],
    /// What beats in the corner: the heart, or V1 for ULTRAKILL (asked 2026-10-08).
    pub sprite: Sprite,
    pub vehicle: VehicleStyle,
    /// The loopback port the Counter-Strike 2 listener takes.
    pub cs2_port: u16,
    /// The console log a `health log` profile follows; the newest file whose name
    /// starts with it (Zandronum adds a timestamp to the name).
    pub log_file: PathBuf,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            bands: Band::defaults(),
            hold: 3.0,
            calm: 2.0,
            racing: 0.8,
            swell: 0.4,
            flash: Flash::Panel,
            heart: [6, 5],
            readout: [157, 2],
            sprite: Sprite::Heart,
            vehicle: VehicleStyle::Gear,
            cs2_port: CS2_PORT,
            log_file: crate::state_dir().join("game.log"),
        }
    }
}

/// The sprite in the corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sprite {
    Heart,
    V1,
}

impl Tuning {
    pub fn path() -> PathBuf {
        crate::config_dir().join("meter")
    }
    /// A profile's own lines, layered over the file: `~/.config/g13map/meter.d/NAME`
    /// (asked 2026-10-08: a look per game).
    pub fn profile_path(profile: &str) -> PathBuf {
        crate::config_dir().join("meter.d").join(profile)
    }
    pub fn load() -> Tuning {
        fs::read_to_string(Self::path())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
    /// The file, then the profile's lines after it, so they win.
    pub fn load_for(profile: &str) -> Tuning {
        let mut text = fs::read_to_string(Self::path()).unwrap_or_default();
        if let Ok(more) = fs::read_to_string(Self::profile_path(profile)) {
            text.push('\n');
            text.push_str(&more);
        }
        Self::parse(&text)
    }
    /// When either file last changed, for the watcher's re-read.
    pub fn stamp(profile: &str) -> (Option<SystemTime>, Option<SystemTime>) {
        let m = |p: PathBuf| fs::metadata(p).ok()?.modified().ok();
        (m(Self::path()), m(Self::profile_path(profile)))
    }
    pub fn parse(text: &str) -> Tuning {
        let mut t = Tuning::default();
        // The first `band` line replaces the default ladder (death stays).
        let mut listed = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut words = line.split_whitespace();
            let Some(key) = words.next() else { continue };
            let values: Vec<&str> = words.collect();
            let secs = |v: &[&str]| v.first().and_then(|s| s.parse::<f32>().ok());
            let ints = |v: &[&str]| {
                v.iter()
                    .map(|s| s.parse::<i32>().ok())
                    .collect::<Option<Vec<i32>>>()
            };
            match key {
                "band" => {
                    let Some((name, nums)) = values.split_first() else {
                        continue;
                    };
                    let Some(n) = ints(nums) else { continue };
                    let [from, r, g, b] = n[..] else { continue };
                    if !(0..=999).contains(&from)
                        || [r, g, b].iter().any(|c| !(0..=255).contains(c))
                    {
                        continue;
                    }
                    if !listed {
                        t.bands.retain(|b| b.from == 0);
                        listed = true;
                    }
                    t.bands.retain(|b| b.from != from as u32);
                    t.bands
                        .push(Band::new(name, from as u32, [r as u8, g as u8, b as u8]));
                }
                "heart" => {
                    if let Some([x, y]) = ints(&values).as_deref() {
                        // On the panel; rings may run off its edge, the file's choice.
                        t.heart = [(*x).clamp(0, W as i32 - 16), (*y).clamp(0, H as i32 - 14)];
                    }
                }
                "readout" => {
                    if let Some([right, top]) = ints(&values).as_deref() {
                        t.readout = [(*right).clamp(34, W as i32), (*top).clamp(0, H as i32 - 14)];
                    }
                }
                "hold" => {
                    t.hold = secs(&values)
                        .filter(|s| (0.0..=60.0).contains(s))
                        .unwrap_or(t.hold)
                }
                "calm" => {
                    t.calm = secs(&values)
                        .filter(|s| (0.2..=60.0).contains(s))
                        .unwrap_or(t.calm)
                }
                "racing" => {
                    t.racing = secs(&values)
                        .filter(|s| (0.2..=60.0).contains(s))
                        .unwrap_or(t.racing)
                }
                "swell" => {
                    t.swell = secs(&values)
                        .filter(|s| (0.1..=10.0).contains(s))
                        .unwrap_or(t.swell)
                }
                "flash" => {
                    t.flash = match values.first() {
                        Some(&"on" | &"panel") => Flash::Panel,
                        Some(&"trace") => Flash::Trace,
                        Some(&"off") => Flash::Off,
                        _ => t.flash,
                    }
                }
                "sprite" => {
                    t.sprite = match values.first() {
                        Some(&"heart") => Sprite::Heart,
                        Some(&"v1") => Sprite::V1,
                        _ => t.sprite,
                    }
                }
                "vehicle" => {
                    t.vehicle = match values.first() {
                        Some(&"tread") => VehicleStyle::Tread,
                        Some(&"gear") => VehicleStyle::Gear,
                        Some(&"scan") => VehicleStyle::Scan,
                        _ => t.vehicle,
                    }
                }
                "cs2_port" => {
                    t.cs2_port = values
                        .first()
                        .and_then(|v| v.parse().ok())
                        .filter(|p| *p > 0)
                        .unwrap_or(t.cs2_port)
                }
                "log_file" => {
                    if let Some(v) = values.first() {
                        t.log_file = PathBuf::from(v);
                    }
                }
                _ => {
                    // A band's name with a colour recolours it (the file's first grammar:
                    // `red 255 0 0`).
                    if let Some(band) = t.bands.iter_mut().find(|b| b.name == key) {
                        let rgb: Vec<u8> = values.iter().filter_map(|v| v.parse().ok()).collect();
                        if let [r, g, b] = rgb[..] {
                            band.rgb = [r, g, b];
                        }
                    }
                }
            }
        }
        t.bands.sort_by_key(|b| b.from);
        if t.bands.first().is_none_or(|b| b.from != 0) {
            t.bands.insert(0, Band::new("dead", 0, [0, 0, 0]));
        }
        t
    }
    /// The file, commented, as `prepare` writes it the first time.
    pub fn to_text(&self) -> String {
        let mut s = String::from(
            "# The G13 health meter's look. g13map watch re-reads this within two seconds.\n\
             # Backlight bands, lowest first: band NAME FROM R G B, the health the band\n\
             # starts at and its colour, 0-255. The band at 0 is death (lights out, a\n\
             # flatline); the lowest above it is the alarm, whose beats flash. Add one for\n\
             # a game whose health runs past 200: band purple 201 160 0 255\n",
        );
        for band in &self.bands {
            let [r, g, b] = band.rgb;
            s.push_str(&format!("band {} {} {r} {g} {b}\n", band.name, band.from));
        }
        s.push_str(&format!(
            "# Seconds the dark flatline holds after a drop to zero.\nhold {}\n\
             # Seconds from beat to beat at full health, and on the last point.\ncalm {}\nracing {}\n\
             # Seconds the heart swells on each beat.\nswell {}\n\
             # What the alarm band's beats flash: panel, trace (the trace and the heart) or off.\nflash {}\n\
             # The heart's top-left corner, in pixels of the 160x43 panel.\nheart {} {}\n\
             # The readout's right edge and top.\nreadout {} {}\n\
             # What beats in the corner: heart, or v1 (ULTRAKILL).\nsprite {}\n\
             # A profile's own lines go in meter.d/PROFILE, read after this file.\n\
             # The loopback port for a profile with `health cs2` (the game's cfg must match).\ncs2_port {}\n\
             # The console log a profile with `health log` follows (newest file starting with it).\nlog_file {}\n",
            self.hold,
            self.calm,
            self.racing,
            self.swell,
            match self.flash {
                Flash::Panel => "panel",
                Flash::Trace => "trace",
                Flash::Off => "off",
            },
            self.heart[0],
            self.heart[1],
            self.readout[0],
            self.readout[1],
            match self.sprite {
                Sprite::Heart => "heart",
                Sprite::V1 => "v1",
            },
            self.cs2_port,
            self.log_file.display()
        ));
        s.push_str(&format!(
            "# Vehicle animation: tread, gear or scan.\nvehicle {}\n",
            match self.vehicle {
                VehicleStyle::Tread => "tread",
                VehicleStyle::Gear => "gear",
                VehicleStyle::Scan => "scan",
            }
        ));
        s
    }
    /// The band `pct` falls in: the highest one starting at or below it.
    pub fn band_at(&self, pct: u32) -> &Band {
        self.bands
            .iter()
            .rev()
            .find(|b| b.from <= pct)
            .unwrap_or(&self.bands[0])
    }
    pub fn rgb_at(&self, pct: u32) -> [u8; 3] {
        self.band_at(pct).rgb
    }
    /// The alarm band: the lowest above death.
    pub(super) fn alarm(&self) -> &Band {
        self.bands
            .iter()
            .find(|b| b.from > 0)
            .unwrap_or(&self.bands[0])
    }
    pub(super) fn is_alarm(&self, pct: u32) -> bool {
        pct > 0 && self.band_at(pct).from == self.alarm().from
    }
    /// Damage starts when entering orange and persists in the lower bands.
    pub(super) fn vehicle_damaged(&self, pct: u32) -> bool {
        self.bands
            .iter()
            .find(|b| b.name == "orange")
            .map_or(pct <= 50, |orange| self.band_at(pct).from <= orange.from)
    }
}
