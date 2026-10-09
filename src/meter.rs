// SPDX-License-Identifier: GPL-3.0-or-later
//! The health meter: a live heartbeat on the panel and a backlight colour that follow the
//! player's health in a game, fed through a small file any game adapter can write.
//!
//! The feed is `$XDG_RUNTIME_DIR/g13map-g13-0.health` (`daemon::runtime_file`), one line:
//!
//!   wait                          a game is connected but has no health to report
//!                                 (a lobby, a menu, a spectator seat)
//!   HEALTH[/MAX] [shield S[/MAX]] health (a percentage over 100 is an overshield) and,
//!                                 apart from it, the shield or armour most games put
//!                                 beside health
//!   ... ttl SECONDS               the line expires unless rewritten in time, so a feeder
//!                                 that dies takes its meter with it
//!
//! No file, `off` or an expired line means no game: the profile's own picture and colour
//! come back. `g13map health ...` writes the file; `g13map watch` reads it ten times a
//! second and renders the meter itself, so nothing else needs the daemon's pipe.
//!
//! The backlight follows the health in bands (asked 2026-10-07): green 76–100, yellow
//! 51–75, orange 26–50, red 1–25, blue above 100 (overshield), and off at 0, a flatline;
//! after three seconds dead the panel searches for a pulse like a waiting one, under red
//! (asked 2026-10-07: lights out alone "goes hard"). Waiting itself leaves the profile's
//! colour alone until the game has a pulse.
use crate::draw::*;
use crate::lcd::{Bitmap, H, W};
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[path = "adventure.rs"]
mod adventure;
#[path = "fortress.rs"]
mod fortress;
#[path = "travel.rs"]
mod travel;
pub use adventure::Adventure;
pub use fortress::Fortress;
pub use fortress::{dfhack_install, dfhack_remove};
pub use travel::Travel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)] // bounded Copy labels; avoid per-frame heap ownership
pub enum State {
    /// Connected, no health yet.
    Wait,
    /// Fortress-wide counts, rather than a player's health percentage.
    Fortress(Fortress),
    /// The controlled adventurer's condition, separate from traditional HP.
    Adventure(Adventure),
    /// Live needs and membership while the local Adventure map is unloaded.
    Travel(Travel),
    /// Health and shield, both in percent (health may exceed 100), whether the head
    /// is covered too (CS2's helmet; asked 2026-10-08): the shield bar is solid then,
    /// and whatever else the game has to show.
    Health {
        pct: u32,
        shield: u32,
        helmet: bool,
        extra: Extra,
    },
}

/// What a game may add to the feed line; each word draws its own element and nothing
/// is drawn for a word that is absent (asked 2026-10-08, for ULTRAKILL's V1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Extra {
    /// Vehicle prototype name: primary health belongs to the vehicle while riding.
    pub vehicle: Label,
    /// Character health, percent, while vehicle health is the primary value.
    pub pilot: Option<u32>,
    /// Factorio suit battery charge, percent.
    pub battery: Option<u8>,
    /// Current research progress and its prototype name (one feed word).
    pub research: Option<u8>,
    pub technology: Label,
    /// Active base-attack alerts. The renderer owns the six-second cooldown.
    pub attack: Option<u32>,
    /// `mana CURRENT/MAX`: the current and effective maximum mana.
    pub mana: Option<Resource>,
    /// `defense VALUE`: damage mitigation, not an armour pool.
    pub defense: Option<u32>,
    /// `breath CURRENT/MAX`: remaining air; shown when below maximum.
    pub breath: Option<Resource>,
    /// `cap C`: the health the bar is capped at, percent (hard damage).
    pub cap: Option<u32>,
    /// `rank R`: a style rank, an index into `RANKS`.
    pub rank: Option<u8>,
    /// `style S`: the rank's own meter, percent.
    pub style: Option<u8>,
    /// `time S`: the level timer, whole seconds.
    pub time: Option<u32>,
    /// `dash D`: dashes in tenths (`dash 2.5` is 25; three dashes is 30).
    pub dash: Option<u8>,
    /// `rail R`: a weapon charge, percent.
    pub rail: Option<u8>,
}

impl Extra {
    pub const NONE: Extra = Extra {
        vehicle: Label::EMPTY,
        pilot: None,
        battery: None,
        research: None,
        technology: Label::EMPTY,
        attack: None,
        mana: None,
        defense: None,
        breath: None,
        cap: None,
        rank: None,
        style: None,
        time: None,
        dash: None,
        rail: None,
    };
}

/// Bounded ASCII label in the Copy feed state. Underscores render as spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label([u8; 256]);
impl Default for Label {
    fn default() -> Self {
        Self::EMPTY
    }
}
impl Label {
    pub const EMPTY: Self = Self([0; 256]);
    fn parse(s: &str) -> Option<Self> {
        if s.is_empty()
            || s.len() > 256
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return None;
        }
        let mut label = Self::EMPTY;
        label.0[..s.len()].copy_from_slice(s.as_bytes());
        Some(label)
    }
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0[..self.0.iter().position(|c| *c == 0).unwrap_or(256)])
            .unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resource {
    pub current: u32,
    pub maximum: u32,
}

impl Resource {
    fn parse(value: &str) -> Option<Self> {
        let (current, maximum) = value.split_once('/')?;
        let current = current.parse::<u32>().ok()?;
        let maximum = maximum.parse::<u32>().ok()?;
        if maximum == 0 && current != 0 {
            return None;
        }
        Some(Self { current, maximum })
    }

    fn percent(self) -> u32 {
        if self.maximum == 0 {
            0
        } else {
            ((u64::from(self.current) * 100 + u64::from(self.maximum) / 2)
                / u64::from(self.maximum))
            .min(100) as u32
        }
    }
}

/// The style ranks a feed may name, lowest first (ULTRAKILL's).
pub const RANKS: [&str; 8] = ["D", "C", "B", "A", "S", "SS", "SSS", "U"];

impl State {
    pub fn health(pct: u32) -> State {
        State::Health {
            pct,
            shield: 0,
            helmet: false,
            extra: Extra::NONE,
        }
    }
    /// The health, if there is one to show.
    pub fn pct(self) -> Option<u32> {
        match self {
            State::Wait | State::Fortress(_) | State::Adventure(_) | State::Travel(_) => None,
            State::Health { pct, .. } => Some(pct),
        }
    }
}

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
    fn new(name: &str, from: u32, rgb: [u8; 3]) -> Band {
        Band {
            name: name.into(),
            from,
            rgb,
        }
    }
    /// The G13's LED makes yellow and orange from red plus a little green; full green
    /// in the mix reads lime.
    fn defaults() -> Vec<Band> {
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

/// Who writes the feed while a profile shows the meter (asked 2026-10-07: the meter is
/// a profile's picture, so the window rules put it on the game's windows, and the
/// profile names the reader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    /// Something else writes the feed: a game mod, a script, `g13map health`.
    Feed,
    /// The watcher runs the Counter-Strike 2 Game State listener itself.
    Cs2,
    /// The watcher follows a game's console log file (`log_file` in the tuning file) for
    /// `G13HEALTH HEALTH MAX ARMOR` lines, as the Doom ACS script in `contrib/` prints.
    Log,
}

impl Reader {
    /// The word in a profile's `# health` line and in `g13map profile health`.
    pub fn key(self) -> &'static str {
        match self {
            Reader::Feed => "feed",
            Reader::Cs2 => "cs2",
            Reader::Log => "log",
        }
    }
    pub fn parse(word: &str) -> Option<Reader> {
        [Reader::Feed, Reader::Cs2, Reader::Log]
            .into_iter()
            .find(|r| r.key() == word)
    }
    /// What the reader is, for a label.
    pub fn about(self) -> &'static str {
        match self {
            Reader::Feed => "a mod or script feeds it",
            Reader::Cs2 => "Counter-Strike 2 listener",
            Reader::Log => "a game's console log (Doom)",
        }
    }
    /// The picture name that once selected this reader (`# lcd health cs2`); the watcher
    /// still keys on it inside.
    pub fn picture(self) -> &'static str {
        NAMES
            .iter()
            .find(|(_, r)| *r == self)
            .map_or("health", |(n, _)| n)
    }
}

/// The meter's old picture names (`# lcd health cs2`, before the profile's own
/// `# health` line); a profile that still says one is read as health mode.
pub const NAMES: [(&str, Reader); 3] = [
    ("health", Reader::Feed),
    ("health cs2", Reader::Cs2),
    ("health log", Reader::Log),
];

/// The reader a profile's picture name selects, if it is the meter.
pub fn selects(name: Option<&str>) -> Option<Reader> {
    let name = name?;
    NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, reader)| *reader)
}

/// A still of the meter for a picture name of its own: what the editor shows and what
/// `apply` sends before the watcher takes over.
pub fn preview(name: &str) -> Option<Bitmap> {
    selects(Some(name))?;
    let mut m = Meter::default();
    let state = State::Health {
        pct: 87,
        shield: 60,
        helmet: false,
        extra: Extra::NONE,
    };
    let mut bm = Bitmap::blank();
    for _ in 0..12 {
        bm = m.frame(state);
    }
    Some(bm)
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
    fn alarm(&self) -> &Band {
        self.bands
            .iter()
            .find(|b| b.from > 0)
            .unwrap_or(&self.bands[0])
    }
    fn is_alarm(&self, pct: u32) -> bool {
        pct > 0 && self.band_at(pct).from == self.alarm().from
    }
    /// Damage starts when entering orange and persists in the lower bands.
    fn vehicle_damaged(&self, pct: u32) -> bool {
        self.bands
            .iter()
            .find(|b| b.name == "orange")
            .map_or(pct <= 50, |orange| self.band_at(pct).from <= orange.from)
    }
    /// Notches on the health bar where each band above the alarm begins, up to full.
    fn notches(&self) -> Vec<i32> {
        self.bands
            .iter()
            .filter(|b| b.from > 1 && b.from <= 100)
            .map(|b| BAR_X + BAR_W * (b.from as i32 - 1) / 100)
            .collect()
    }
    fn hold_ticks(&self) -> u32 {
        (self.hold / TICK.as_secs_f32()).round().max(1.0) as u32
    }
    fn swell_ticks(&self) -> u32 {
        (self.swell / TICK.as_secs_f32()).round().max(1.0) as u32
    }
    /// Columns from one beat to the next at `pct`: the trace runs `SCROLL` columns a
    /// tick, so seconds times columns a second, between racing and calm; never shorter than
    /// the complex itself.
    fn period(&self, pct: u32) -> i32 {
        let cols = |secs: f32| (secs * SCROLL as f32 / TICK.as_secs_f32()).round() as i32;
        let (racing, calm) = (cols(self.racing), cols(self.calm));
        (racing + (calm - racing) * pct.min(100) as i32 / 100).max(ECG_COLUMNS)
    }
}

// ---- the feed ----

/// Where `g13map health` writes: the runtime file.
pub fn path() -> Result<PathBuf, String> {
    crate::daemon::runtime_file("health")
}

/// Where feeders may write: the runtime file, and `health` under the state directory
/// (`~/.local/state/g13map/`). A game in Steam's container has a private `/run/user`,
/// only the home directory is shared with the host (found 2026-10-07 with DRG), so a
/// mod inside it reaches the meter through the second one.
pub fn paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = path() {
        v.push(p);
    }
    v.push(crate::state_dir().join("health"));
    v
}

/// Makes the state directory, so a feeder in a container has somewhere to write, and
/// the tuning file with its defaults, commented, if there is none yet.
pub fn prepare() {
    let _ = fs::create_dir_all(crate::state_dir());
    let _ = fs::create_dir_all(crate::config_dir().join("meter.d"));
    let p = Tuning::path();
    if !p.exists() {
        let _ = fs::write(p, Tuning::default().to_text());
    }
}

/// `VALUE` as a whole number of tenths, 0–`max` (for `dash 2.5`).
fn tenths(text: &str, max: f64) -> Option<u8> {
    let v = text.parse::<f64>().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some((v * 10.0).round().min(max * 10.0) as u8)
}

/// The percentage of `text` as `VALUE` or `VALUE/MAX`, rounded, 0–999.
fn percent(text: &str) -> Option<u32> {
    let (v, max) = match text.split_once('/') {
        Some((v, m)) => (v.parse::<f64>().ok()?, m.parse::<f64>().ok()?),
        None => (text.parse::<f64>().ok()?, 100.0),
    };
    if !(v.is_finite() && max.is_finite()) || max <= 0.0 || v < 0.0 {
        return None;
    }
    Some((v / max * 100.0).round().clamp(0.0, 999.0) as u32)
}

/// A feed line, given when it was written: the state, or nothing for off, expired, or a
/// line that does not parse (a broken feeder is no feeder).
pub fn parse(text: &str, written: SystemTime, now: SystemTime) -> Option<State> {
    if text.split_whitespace().next() == Some("travel") {
        return Travel::parse(text, written, now).map(State::Travel);
    }
    if text.split_whitespace().next() == Some("adv") {
        return Adventure::parse(text, written, now).map(State::Adventure);
    }
    if text.split_whitespace().next() == Some("fort") {
        return Fortress::parse(text, written, now).map(State::Fortress);
    }
    let mut words = text.split_whitespace();
    let mut state = match words.next()? {
        "wait" => State::Wait,
        "off" => return None,
        v => State::health(percent(v)?),
    };
    while let Some(key) = words.next() {
        let value = words.next()?;
        match key {
            "shield" => match &mut state {
                State::Health { shield, .. } => *shield = percent(value)?,
                _ => return None,
            },
            "helmet" => match (&mut state, value) {
                (State::Health { helmet, .. }, "on") => *helmet = true,
                (State::Health { helmet, .. }, "off") => *helmet = false,
                _ => return None,
            },
            "cap" | "rank" | "style" | "time" | "dash" | "rail" | "mana" | "defense" | "breath"
            | "battery" | "research" | "technology" | "attack" | "vehicle" | "pilot" => {
                let State::Health { extra, .. } = &mut state else {
                    return None;
                };
                match key {
                    "vehicle" => extra.vehicle = Label::parse(value)?,
                    "pilot" => extra.pilot = Some(percent(value)?),
                    "battery" => extra.battery = Some(percent(value)?.min(100) as u8),
                    "research" => extra.research = Some(percent(value)?.min(100) as u8),
                    "technology" => extra.technology = Label::parse(value)?,
                    "attack" => extra.attack = Some(value.parse::<u32>().ok()?.min(999)),
                    "cap" => extra.cap = Some(percent(value)?),
                    "rank" => extra.rank = Some(RANKS.iter().position(|r| *r == value)? as u8),
                    "style" => extra.style = Some(percent(value)?.min(100) as u8),
                    "time" => {
                        let s = value
                            .parse::<f64>()
                            .ok()
                            .filter(|s| s.is_finite() && *s >= 0.0)?;
                        extra.time = Some(s.min(359_999.0) as u32);
                    }
                    "dash" => extra.dash = Some(tenths(value, 3.0)?),
                    "mana" => extra.mana = Some(Resource::parse(value)?),
                    "breath" => extra.breath = Some(Resource::parse(value)?),
                    "defense" => extra.defense = Some(value.parse::<u32>().ok()?.min(999)),
                    _ => extra.rail = Some(percent(value)?.min(100) as u8),
                }
            }
            "ttl" => {
                let ttl = Duration::try_from_secs_f64(value.parse::<f64>().ok()?).ok()?;
                if ttl.is_zero() || now.duration_since(written).unwrap_or_default() > ttl {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(state)
}

/// The state of the most recently written feed file that has one.
pub fn read() -> Option<State> {
    let now = SystemTime::now();
    paths()
        .into_iter()
        .filter_map(|p| {
            let written = fs::metadata(&p).ok()?.modified().ok()?;
            let state = parse(&fs::read_to_string(&p).ok()?, written, now)?;
            Some((written, state))
        })
        .max_by_key(|(written, _)| *written)
        .map(|(_, state)| state)
}

/// Writes the feed (atomically: beside, then renamed) or, for `None`, removes every
/// feed file: no game.
pub fn write(state: Option<State>, ttl: Option<Duration>) -> Result<(), String> {
    let ttl = ttl.or_else(|| {
        matches!(
            state,
            Some(State::Fortress(_) | State::Adventure(_) | State::Travel(_))
        )
        .then(|| Duration::from_secs(6))
    });
    let p = path()?;
    let Some(state) = state else {
        for p in paths() {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        return Ok(());
    };
    let mut line = match state {
        State::Wait => "wait".to_string(),
        State::Fortress(fort) => fort.to_line(),
        State::Adventure(adv) => adv.to_line(),
        State::Travel(travel) => travel.to_line(),
        State::Health {
            pct,
            shield,
            helmet,
            extra,
        } => {
            let mut s = format!(
                "{pct} shield {shield}{}",
                if helmet { " helmet on" } else { "" }
            );
            if !extra.vehicle.as_str().is_empty() {
                s.push_str(&format!(" vehicle {}", extra.vehicle.as_str()));
            }
            if let Some(pilot) = extra.pilot {
                s.push_str(&format!(" pilot {pilot}"));
            }
            if let Some(c) = extra.cap {
                s.push_str(&format!(" cap {c}"));
            }
            if let Some(r) = extra.rank {
                s.push_str(&format!(" rank {}", RANKS[r as usize % RANKS.len()]));
            }
            if let Some(v) = extra.style {
                s.push_str(&format!(" style {v}"));
            }
            if let Some(t) = extra.time {
                s.push_str(&format!(" time {t}"));
            }
            if let Some(d) = extra.dash {
                s.push_str(&format!(" dash {}.{}", d / 10, d % 10));
            }
            if let Some(r) = extra.rail {
                s.push_str(&format!(" rail {r}"));
            }
            if let Some(m) = extra.mana {
                s.push_str(&format!(" mana {}/{}", m.current, m.maximum));
            }
            if let Some(d) = extra.defense {
                s.push_str(&format!(" defense {d}"));
            }
            if let Some(b) = extra.breath {
                s.push_str(&format!(" breath {}/{}", b.current, b.maximum));
            }
            if let Some(b) = extra.battery {
                s.push_str(&format!(" battery {b}"));
            }
            if let Some(r) = extra.research {
                s.push_str(&format!(" research {r}"));
            }
            if !extra.technology.as_str().is_empty() {
                s.push_str(&format!(" technology {}", extra.technology.as_str()));
            }
            if let Some(a) = extra.attack {
                s.push_str(&format!(" attack {a}"));
            }
            s
        }
    };
    if let Some(ttl) = ttl {
        line.push_str(&format!(" ttl {}", ttl.as_secs().max(1)));
    }
    line.push('\n');
    let tmp = p.with_extension("health.new");
    fs::write(&tmp, line)
        .and_then(|_| fs::rename(&tmp, &p))
        .map_err(|e| format!("{}: {e}", p.display()))
}

// ---- the picture ----

/// Frames come every tick; the trace scrolls this many columns per tick.
pub const TICK: Duration = Duration::from_millis(100);
const SCROLL: i32 = 5;
const BASE: i32 = 22;

#[rustfmt::skip]
const HEART: &[&str] = &[
    ".##.##.", "#######", "#######", ".#####.", "..###..", "...#...",
];
#[rustfmt::skip]
const HEART_BIG: &[&str] = &[
    ".###.###.",
    "#########",
    "#########",
    "#########",
    ".#######.",
    "..#####..",
    "...###...",
    "....#....",
];
#[rustfmt::skip]
const HEART_OUTLINE: &[&str] = &[
    ".##.##.", "#..#..#", "#.....#", ".#...#.", "..#.#..", "...#...",
];

/// V1, ULTRAKILL's machine, 15 by 13: the fins, the visor, the jaw. Blinks on the beat.
#[rustfmt::skip]
const V1: &[&str] = &[
    "#.....###.....#",
    "##...#####...##",
    "###.#######.###",
    "####.#####.####",
    ".#####...#####.",
    "..###########..",
    "..#.........#..",
    "..#..#####..#..",
    "..#.........#..",
    "...#########...",
    "....#.....#....",
    "....##...##....",
    ".....#####.....",
];
#[rustfmt::skip]
const V1_BLINK: &[&str] = &[
    "#.....###.....#",
    "##...#####...##",
    "###.#######.###",
    "####.#####.####",
    ".#####...#####.",
    "..###########..",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "...#########...",
    "....#.....#....",
    "....##...##....",
    ".....#####.....",
];
#[rustfmt::skip]
const V1_OUTLINE: &[&str] = &[
    "#.....###.....#",
    "##...#...#...##",
    "#.#.#.....#.#.#",
    "#..#.......#..#",
    ".#...........#.",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "..#.........#..",
    "...#.......#...",
    "....#.....#....",
    "....#.....#....",
    ".....#####.....",
];

/// Letters the feed may put on the panel, including arbitrary research names.
#[rustfmt::skip]
const LETTERS: [(char, [&str; 7]); 36] = [
    ('A', [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('B', ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."]),
    ('C', [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."]),
    ('D', ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."]),
    ('S', [".####", "#....", "#....", ".###.", "....#", "....#", "####."]),
    ('U', ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    (':', [".....", "..#..", "..#..", ".....", "..#..", "..#..", "....."]),
    ('.', [".....", ".....", ".....", ".....", ".....", "..#..", "..#.."]),
    ('M', ["#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#"]),
    ('P', ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."]),
    ('E', ["#####", "#....", "#....", "####.", "#....", "#....", "#####"]),
    ('F', ["#####", "#....", "#....", "####.", "#....", "#....", "#...."]),
    ('I', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"]),
    ('R', ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"]),
    ('G', [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###."]),
    ('H', ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"]),
    ('J', ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."]),
    ('K', ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"]),
    ('L', ["#....", "#....", "#....", "#....", "#....", "#....", "#####"]),
    ('N', ["#...#", "##..#", "##..#", "#.#.#", "#..##", "#..##", "#...#"]),
    ('O', [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."]),
    ('Q', [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"]),
    ('T', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."]),
    ('V', ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."]),
    ('W', ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"]),
    ('X', ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"]),
    ('Y', ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."]),
    ('Z', ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"]),
    ('/', ["....#", "....#", "...#.", "..#..", ".#...", "#....", "#...."]),
    (' ', [".....", ".....", ".....", ".....", ".....", ".....", "....."]),
    ('%', ["##..#", "##..#", "...#.", "..#..", ".#...", "#..##", "#..##"]),
    ('(', ["...#.", "..#..", ".#...", ".#...", ".#...", "..#..", "...#."]),
    (')', [".#...", "..#..", "...#.", "...#.", "...#.", "..#..", ".#..."]),
    ('°', [".###.", ".#.#.", ".###.", ".....", ".....", ".....", "....."]),
    ('?', [".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#.."]),
    ('+', [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."]),
];

/// The 5x7 glyph for `c`: a digit, a letter from `LETTERS`, or the dash.
fn glyph(c: char) -> &'static [&'static str; 7] {
    if let Some(d) = c.to_digit(10) {
        return &GLYPHS[d as usize];
    }
    LETTERS
        .iter()
        .find(|(k, _)| *k == c)
        .map_or(&GLYPHS[10], |(_, g)| g)
}

/// `text` in 5x7 glyphs at `scale`, top-left at `x, y`; returns the width drawn.
fn text(bm: &mut Bitmap, x: i32, y: i32, text: &str, scale: i32) -> i32 {
    for (i, c) in text.chars().enumerate() {
        for (r, row) in glyph(c).iter().enumerate() {
            for (k, cell) in row.bytes().enumerate() {
                if cell == b'#' {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            bm.plot(
                                x + (i as i32 * 6 + k as i32) * scale + dx,
                                y + r as i32 * scale + dy,
                            );
                        }
                    }
                }
            }
        }
    }
    (text.chars().count() as i32 * 6 - 1) * scale
}

/// The level timer as `m:ss`, or `h:mm:ss` past an hour (a hard boss can take three).
fn clock(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 5x7 digits and a dash, drawn at twice the size for the readout.
#[rustfmt::skip]
const GLYPHS: [[&str; 7]; 11] = [
    [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
    ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
    [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
    ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
    ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
    ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
    ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
    ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
    [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
    [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
    [".....", ".....", ".....", "#####", ".....", ".....", "....."],
];

/// The readout, right-aligned to `right`, 2x glyphs, on a darkened patch.
fn readout(bm: &mut Bitmap, text: &str, right: i32, y: i32) {
    let w = text.len() as i32 * 12 - 2;
    let x0 = right - w;
    bm.halo(x0, y, w, 14);
    self::text(bm, x0, y, text, 2);
}

/// A small gauge: a box `w` by `h` with `fill` (0–100) of its inside lit.
fn gauge(bm: &mut Bitmap, x: i32, y: i32, w: i32, h: i32, fill: u32) {
    bm.halo(x, y, w, h);
    bm.line(x, y, x + w - 1, y);
    bm.line(x, y + h - 1, x + w - 1, y + h - 1);
    bm.line(x, y, x, y + h - 1);
    bm.line(x + w - 1, y, x + w - 1, y + h - 1);
    let f = ((w - 4) * fill.min(100) as i32 + 50) / 100;
    for xx in x + 2..x + 2 + f {
        for yy in y + 2..y + h - 2 {
            bm.plot(xx, yy);
        }
    }
}

/// Eight teeth and three cutouts remain legible in the small vehicle icon.
/// Cracks rotate with the metal, rather than flickering with the heartbeat.
fn gear(bm: &mut Bitmap, cx: i32, cy: i32, angle: f32, damaged: bool) {
    use std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    for dy in -6..=6 {
        for dx in -6..=6 {
            let radius = ((dx * dx + dy * dy) as f32).sqrt();
            let theta = (dy as f32).atan2(dx as f32) - angle;
            let tooth = (theta + TAU / 16.0).rem_euclid(TAU / 8.0) - TAU / 16.0;
            let edge = 4.4 + 1.6 * (1.0 - (tooth.abs() - 0.11).max(0.0) / 0.13).max(0.0);
            let hole = (theta + TAU / 6.0).rem_euclid(TAU / 3.0) - TAU / 6.0;
            let along = dx as f32 * cos + dy as f32 * sin;
            let across = (-dx as f32 * sin + dy as f32 * cos) * along.signum();
            let bend = if (2.6..4.1).contains(&along.abs()) {
                1.0
            } else {
                0.0
            };
            // A two-pixel-wide zigzag splits the metal all the way through its
            // rim. Mask this wheel before compositing, so a crack cannot erase
            // a neighbouring wheel at the interlocking teeth.
            let crack = damaged && along.abs() > 1.0 && (across - bend).abs() <= 1.0;
            if radius > 1.25
                && radius <= edge
                && !(radius > 2.0 && radius < 3.8 && hole.abs() < 0.22)
                && !crack
            {
                bm.plot(cx + dx, cy + dy);
            }
        }
    }
}

const BAR_X: i32 = 4;
const BAR_W: i32 = 152;

/// The shield bar's top row; its second hundred adds a row above and below.
const SHIELD_TOP: i32 = 30;

/// The ring around a sprite `w` by `h` at `hx, hy`, `out` pixels further out than the
/// first, with its corners cut.
fn ring(bm: &mut Bitmap, hx: i32, hy: i32, w: i32, h: i32, out: i32) {
    let (l, t, r, b) = (
        hx - 4 - out,
        hy - 3 - out,
        hx + w + 3 + out,
        hy + h + 2 + out,
    );
    bm.line(l + 1, t, r - 1, t);
    bm.line(l + 1, b, r - 1, b);
    bm.line(l, t + 1, l, b - 1);
    bm.line(r, t + 1, r, b - 1);
}

/// The health bar along the bottom with notches at the band edges, and the shield bar
/// above it when there is any shield: hatched, or solid when the head is covered too.
/// `shield` may run to twice the bar: the second hundred thickens it from the left.
/// `cap` hatches the bar from there to full: health the game has taken away for now.
fn bars(bm: &mut Bitmap, fill: i32, shield: i32, solid: bool, notches: &[i32], cap: Option<i32>) {
    if let Some(cap) = cap {
        for x in BAR_X + cap.clamp(0, BAR_W)..BAR_X + BAR_W {
            for y in 36..=40 {
                if (x + y) % 2 == 0 {
                    bm.plot(x, y);
                }
            }
        }
    }
    for x in 2..=157 {
        bm.plot(x, 34);
        bm.plot(x, 42);
    }
    for y in 34..=42 {
        bm.plot(2, y);
        bm.plot(157, y);
    }
    for &x in notches {
        bm.put(x, 34, false);
        bm.put(x, 42, false);
    }
    for x in BAR_X..BAR_X + fill.clamp(0, BAR_W) {
        for y in 36..=40 {
            bm.plot(x, y);
        }
    }
    let hatch = |bm: &mut Bitmap, x: i32, y: i32| {
        if solid || (x + y) % 2 == 0 {
            bm.plot(x, y);
        }
    };
    for x in BAR_X..BAR_X + shield.clamp(0, BAR_W) {
        for y in SHIELD_TOP..=SHIELD_TOP + 2 {
            hatch(bm, x, y);
        }
    }
    for x in BAR_X..BAR_X + (shield - BAR_W).clamp(0, BAR_W) {
        hatch(bm, x, SHIELD_TOP - 1);
        hatch(bm, x, SHIELD_TOP + 3);
    }
}

/// The meter's picture as it runs: the trace as a ring of column lifts, scrolled left
/// `SCROLL` columns a tick with new columns from the beat clock on the right. A pure
/// function of the ticks and states it was given, so tests can draw it without a clock.
pub struct Meter {
    fortress: fortress::Display,
    attack_flash: Option<Instant>,
    lift: [i32; W],
    /// Columns since the current beat began.
    u: i32,
    /// Ticks since the last beat began; `None` before the first.
    beat_age: Option<u32>,
    /// Ticks at zero health.
    dead: u32,
    tick: u32,
    research_name: Label,
    research_started: u32,
    /// The look; the frame thread takes a new one from the file.
    pub tuning: Tuning,
}

impl Default for Meter {
    fn default() -> Self {
        Meter {
            fortress: fortress::Display::default(),
            attack_flash: None,
            lift: [0; W],
            u: 0,
            beat_age: None,
            dead: 0,
            tick: 0,
            research_name: Label::EMPTY,
            research_started: 0,
            tuning: Tuning::default(),
        }
    }
}

impl Meter {
    /// Advances one tick in `state` and draws it.
    pub fn frame(&mut self, state: State) -> Bitmap {
        self.frame_at(state, Instant::now())
    }

    fn frame_at(&mut self, state: State, now: Instant) -> Bitmap {
        let mut bm = Bitmap::blank();
        self.tick = self.tick.wrapping_add(1);
        if let State::Fortress(fort) = state {
            self.dead = 0;
            self.fortress.draw(&mut bm, fort, now);
            return bm;
        }
        if let State::Adventure(adv) = state {
            self.dead = 0;
            adv.draw(&mut self.fortress, &mut bm, now);
            return bm;
        }
        if let State::Travel(travel) = state {
            self.dead = 0;
            travel.draw(&mut self.fortress, &mut bm, now);
            return bm;
        }
        // A drop to zero always holds the dark flatline for the whole hold, whatever the
        // feed says meanwhile (asked 2026-10-07); only then does the next state show.
        let zero =
            matches!(state, State::Health { pct: 0, extra, .. } if extra.vehicle == Label::EMPTY);
        self.dead = if zero || self.holding() {
            self.dead + 1
        } else {
            0
        };
        match state {
            _ if self.holding() => self.alive(&mut bm, 0, 0, false, Extra::NONE),
            State::Fortress(fort) => self.fortress.draw(&mut bm, fort, now),
            State::Adventure(adv) => adv.draw(&mut self.fortress, &mut bm, now),
            State::Travel(travel) => travel.draw(&mut self.fortress, &mut bm, now),
            State::Wait => self.waiting(&mut bm),
            State::Health {
                pct, shield, extra, ..
            } if extra.vehicle != Label::EMPTY => {
                self.vehicle(&mut bm, pct, shield, extra);
            }
            State::Health { pct: 0, .. } => self.waiting(&mut bm),
            State::Health {
                pct,
                shield,
                helmet,
                extra,
            } => self.alive(&mut bm, pct, shield, helmet, extra),
        }
        if let State::Health { extra, .. } = state {
            if extra.attack.unwrap_or(0) > 0 && !self.holding() {
                if self
                    .attack_flash
                    .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(6))
                {
                    self.attack_flash = Some(now);
                }
                if self
                    .attack_flash
                    .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(300))
                {
                    // Fill the entire visible glass, including the bars. A steady label
                    // remains between flashes; changing alert counts cannot retrigger it.
                    for y in 0..H {
                        for x in 0..W {
                            bm.set(x, y, true);
                        }
                    }
                }
            }
        }
        bm
    }

    /// The corner sprite and its size: the heart, or V1.
    fn sprite(&self, pct: u32) -> (&'static [&'static str], i32, i32, i32, i32) {
        let swell = self.beat_age.is_some_and(|a| a < self.tuning.swell_ticks());
        match self.tuning.sprite {
            Sprite::Heart => match (pct, swell) {
                (0, _) => (HEART_OUTLINE, 7, 6, 0, 0),
                (_, true) => (HEART_BIG, 7, 6, -1, -1),
                _ => (HEART, 7, 6, 0, 0),
            },
            Sprite::V1 => match (pct, swell) {
                (0, _) => (V1_OUTLINE, 15, 13, 0, 0),
                (_, true) => (V1_BLINK, 15, 13, 0, 0),
                _ => (V1, 15, 13, 0, 0),
            },
        }
    }

    /// Inside the dark seconds after a drop to zero.
    fn holding(&self) -> bool {
        (1..=self.tuning.hold_ticks()).contains(&self.dead)
    }

    /// The backlight for the frame just drawn: off through the hold, the band's colour,
    /// the resting colour while waiting, red once a flatline has turned into the search
    /// for a pulse.
    pub fn colour(&self, state: State, rest: [u8; 3]) -> [u8; 3] {
        if let State::Travel(travel) = state {
            return travel.colour(&self.tuning);
        }
        if let State::Adventure(adv) = state {
            return if adv.severity != 4 && self.fortress.is_flashing() {
                self.tuning.alarm().rgb
            } else {
                adv.colour(&self.tuning)
            };
        }
        if let State::Fortress(fort) = state {
            if let Some(season) = fort.season {
                return if self.fortress.is_flashing() {
                    self.tuning.alarm().rgb
                } else {
                    season.rgb()
                };
            }
            return match fort.severity {
                2 => self.tuning.alarm().rgb,
                1 => self.tuning.rgb_at(60),
                _ => rest,
            };
        }
        if self.holding() {
            return self.tuning.rgb_at(0);
        }
        match state.pct() {
            None => rest,
            Some(0) => self.tuning.alarm().rgb,
            Some(pct) => self.tuning.rgb_at(pct),
        }
    }

    fn scroll(&mut self, pct: u32) {
        let beating = pct > 0;
        let period = self.tuning.period(pct);
        let mut beat = false;
        self.lift.rotate_left(SCROLL as usize);
        for x in W - SCROLL as usize..W {
            self.lift[x] = if beating { ecg(self.u) } else { 0 };
            // The heart pumps as the R peak enters, not as the quiet P wave does.
            beat |= beating && self.u == ECG_R;
            // A flat trace holds the clock at the start, so the next pulse begins whole.
            self.u = if beating && self.u + 1 < period {
                self.u + 1
            } else {
                0
            };
        }
        self.beat_age = match (beat, self.beat_age) {
            (true, _) => Some(0),
            (false, Some(a)) => Some(a + 1),
            (false, None) => None,
        };
    }

    fn trace(&self, bm: &mut Bitmap) {
        for x in 0..W as i32 - 1 {
            let (a, b) = (self.lift[x as usize], self.lift[x as usize + 1]);
            bm.line(x, BASE - a, x + 1, BASE - b);
        }
    }

    fn alive(&mut self, bm: &mut Bitmap, pct: u32, shield: u32, helmet: bool, extra: Extra) {
        self.scroll(pct);
        self.trace(bm);
        let [hx, hy] = self.tuning.heart;
        let (sprite, w, h, dx, dy) = self.sprite(pct);
        if self.tuning.sprite == Sprite::V1 {
            bm.halo(hx, hy, w, h);
        }
        bm.sprite(hx + dx, hy + dy, sprite);
        if shield > 0 {
            // Rings around the heart, the shield's own mark: one per hundred, up to three
            // (Doom's blue armour is 200 and mods go past it; asked 2026-10-08).
            for out in 0..shield.div_ceil(100).min(3) {
                ring(bm, hx, hy, w, h, out as i32);
            }
        }
        // Under the sprite: the dashes as pips, a charge as a small gauge.
        let under = hy + h + 2;
        if let Some(d) = extra.dash {
            bm.halo(hx - 3, under, 16, 4);
            for i in 0..3u8 {
                let x = hx - 3 + i as i32 * 6;
                let rows = match d.saturating_sub(i * 10) {
                    10.. => 4,
                    1..=9 => 2,
                    0 => 1,
                };
                for y in under..under + rows {
                    bm.line(x, y, x + 3, y);
                }
            }
        }
        if let Some(r) = extra.rail {
            gauge(bm, hx + 16, under - 1, 20, 6, r as u32);
        }
        // The style rank, its letters big at the top centre with its own meter beneath.
        let [rx, ry] = self.tuning.readout;
        if let Some(rank) = extra.rank {
            let letters = RANKS[rank as usize % RANKS.len()];
            let w = (letters.len() as i32 * 6 - 1) * 2;
            bm.halo(64, ry, w, 14);
            text(bm, 64, ry, letters, 2);
            if let Some(fill) = extra.style {
                gauge(bm, 64, ry + 15, w.max(20), 5, fill as u32);
            }
        }
        readout(bm, &pct.min(999).to_string(), rx, ry);
        if let Some(t) = extra.time {
            let s = clock(t);
            let w = s.len() as i32 * 6 - 1;
            bm.halo(rx - w, ry + 16, w, 7);
            text(bm, rx - w, ry + 16, &s, 1);
        }
        if let Some(mana) = extra.mana {
            let label = format!("MP {}/{}", mana.current, mana.maximum);
            // Keep large modded pools inside this slot without obscuring health.
            let label = if label.len() > 11 {
                format!("MP {}%", mana.percent())
            } else {
                label
            };
            bm.halo(43, 2, 65, 14);
            text(bm, 43, 2, &label, 1);
            gauge(bm, 43, 11, 65, 6, mana.percent());
        }
        if let Some(defense) = extra.defense {
            bm.halo(43, 19, 41, 7);
            text(bm, 43, 19, &format!("DEF {defense}"), 1);
        }
        if let Some(breath) = extra.breath.filter(|b| b.current < b.maximum) {
            bm.halo(88, 19, 50, 7);
            text(bm, 88, 19, "AIR", 1);
            gauge(bm, 109, 20, 29, 5, breath.percent());
        }
        if let Some(battery) = extra.battery {
            bm.halo(43, 2, 65, 15);
            text(bm, 43, 2, &format!("BAT {battery}%"), 1);
            gauge(bm, 43, 11, 65, 6, battery.into());
        }
        if let Some(attacks) = extra.attack.filter(|n| *n > 0) {
            bm.halo(43, 19, 95, 7);
            text(bm, 43, 19, &format!("ATTACK {attacks}"), 1);
        } else if let Some(research) = extra.research {
            bm.halo(43, 19, 95, 7);
            self.research(bm, 43, 19, extra.technology, research);
        }
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            (BAR_W * shield.min(200) as i32 + 50) / 100,
            helmet,
            &self.tuning.notches(),
            extra.cap.map(|c| (BAR_W * c.min(100) as i32 + 50) / 100),
        );
        // In the alarm band every beat flashes: the panel, or the trace alone.
        if self.beat_age == Some(0) && self.tuning.is_alarm(pct) {
            let rows = match self.tuning.flash {
                Flash::Off => 0,
                Flash::Trace => SHIELD_TOP as usize - 1,
                Flash::Panel => H,
            };
            for y in 0..rows {
                for x in 0..W {
                    bm.set(x, y, !bm.get(x, y));
                }
            }
        }
    }

    fn vehicle(&mut self, bm: &mut Bitmap, pct: u32, shield: u32, extra: Extra) {
        // Returning on foot starts a fresh ECG instead of retaining old beats.
        self.lift.fill(0);
        self.u = 0;
        self.beat_age = None;
        let phase = (self.tick % 48) as i32;
        match self.tuning.vehicle {
            VehicleStyle::Tread => {
                // A steady hull with circulating tread links, independent of HP.
                bm.line(5, 9, 25, 9);
                bm.line(5, 16, 25, 16);
                bm.line(5, 9, 3, 12);
                bm.line(3, 12, 5, 16);
                bm.line(25, 9, 27, 12);
                bm.line(27, 12, 25, 16);
                bm.line(8, 7, 22, 7);
                bm.line(8, 7, 8, 9);
                bm.line(22, 7, 22, 9);
                bm.line(13, 4, 19, 4);
                bm.line(13, 4, 13, 7);
                bm.line(19, 4, 19, 7);
                bm.line(19, 5, 27, 5);
                for x in 5..=25 {
                    if (x + phase / 2) % 4 < 2 {
                        bm.plot(x, 11);
                        bm.plot(30 - x, 14);
                    }
                }
            }
            VehicleStyle::Gear => {
                let angle = phase as f32 * std::f32::consts::TAU / 48.0;
                let damaged = self.tuning.vehicle_damaged(pct);
                // Adjacent wheels counter-rotate; the middle meshes with both.
                gear(bm, 7, 6, angle, damaged);
                gear(bm, 16, 12, -angle, damaged);
                gear(bm, 25, 6, angle, damaged);
            }
            VehicleStyle::Scan => {
                gauge(bm, 3, 2, 26, 18, 0);
                let step = phase % 48;
                let x = 4 + if step < 24 { step } else { 47 - step };
                bm.line(x, 5, x, 16);
                bm.line(x - 1, 7, x - 1, 14);
            }
        }
        let label = extra
            .vehicle
            .as_str()
            .replace(['-', '_'], " ")
            .to_uppercase();
        text(bm, 33, 2, &format!("{label:.13}"), 1);
        readout(bm, &pct.min(999).to_string(), 157, 2);
        if let Some(pilot) = extra.pilot {
            text(bm, 33, 12, &format!("HP {pilot}%"), 1);
        }
        text(bm, 3, 23, &format!("SH {shield}%"), 1);
        if let Some(battery) = extra.battery {
            text(bm, 75, 12, &format!("BAT {battery}%"), 1);
        }
        if let Some(attacks) = extra.attack.filter(|n| *n > 0) {
            text(bm, 55, 23, &format!("ATTACK {attacks}"), 1);
        } else if let Some(research) = extra.research {
            self.research(bm, 55, 23, extra.technology, research);
        }
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            0,
            false,
            &self.tuning.notches(),
            None,
        );
    }

    fn research(&mut self, bm: &mut Bitmap, x: i32, y: i32, label: Label, progress: u8) {
        if label != self.research_name {
            self.research_name = label;
            self.research_started = self.tick;
        }
        let name = label.as_str();
        let name = if name.is_empty() { "SCIENCE" } else { name };
        let name = name.replace(['-', '_'], " ").to_uppercase();
        const WIDTH: i32 = 53; // nine small glyphs, with progress in a separate slot
        let length = name.len() as i32 * 6 - 1;
        if length <= WIDTH {
            text(bm, x, y, &name, 1);
        } else {
            let loop_width = length + 19; // three spaces between copies
            let cycle = 12 + (loop_width as u32).div_ceil(2);
            let phase = self.tick.wrapping_sub(self.research_started) % cycle;
            let offset = phase.saturating_sub(12) as i32 * 2;
            let mut clipped = Bitmap::blank();
            text(&mut clipped, -offset, 0, &name, 1);
            text(&mut clipped, loop_width - offset, 0, &name, 1);
            for yy in 0..7 {
                for xx in 0..WIDTH {
                    bm.put(x + xx, y + yy, clipped.get(xx as usize, yy as usize));
                }
            }
        }
        text(bm, x + 56, y, &format!("{progress}%"), 1);
    }

    /// Connected, waiting for a pulse: a monitor sweep erasing a flat line ahead of its
    /// cursor, the heart as an outline that fills for a moment now and then, a dashed
    /// readout and a mark pacing the empty bar.
    fn waiting(&mut self, bm: &mut Bitmap) {
        self.scroll(0);
        let t = self.tick as i32;
        let cursor = (t * SCROLL) % (W as i32 + 16);
        for x in 0..W as i32 {
            if x < cursor - 12 || x > cursor {
                bm.plot(x, BASE);
            }
        }
        bm.line(cursor, BASE - 2, cursor, BASE + 2);
        let [hx, hy] = self.tuning.heart;
        let full = t % 20 < 3;
        match self.tuning.sprite {
            Sprite::Heart => bm.sprite(hx, hy, if full { HEART } else { HEART_OUTLINE }),
            Sprite::V1 => {
                bm.halo(hx, hy, 15, 13);
                bm.sprite(hx, hy, if full { V1 } else { V1_OUTLINE });
            }
        }
        let [rx, ry] = self.tuning.readout;
        readout(bm, "--", rx, ry);
        bars(bm, 0, 0, false, &self.tuning.notches(), None);
        let x = BAR_X + tri(t * 2, BAR_W - 6);
        for dx in 0..6 {
            for y in 36..=40 {
                bm.plot(x + dx, y);
            }
        }
    }
}

// ---- the meter on the panel ----

/// What the frame thread follows: the state, the resting colour, and a request to send
/// the colour again after something else wrote the backlight.
struct Shared {
    state: State,
    rest: [u8; 3],
    resend: bool,
    tuning: Tuning,
}

/// The meter running: a thread drawing a frame a tick and sending the backlight colour
/// the band calls for. Everything the pipe gets comes from that one thread, in order.
/// Dropping it stops the frames and puts the resting colour back; the caller puts the
/// picture back.
pub struct Live {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    reader: Reader,
    /// The CS2 listener, for `Reader::Cs2`; dropped with the meter.
    _listener: Option<Listener>,
}

impl Live {
    /// Starts the frames (yielding to an open editor, like a kept animation) and, for
    /// `Reader::Cs2`, the game-state listener.
    pub fn start(state: State, rest: [u8; 3], reader: Reader, profile: &str) -> Live {
        let tuning = Tuning::load_for(profile);
        let listener = match reader {
            Reader::Feed => None,
            Reader::Cs2 => Some(Listener::start(tuning.cs2_port)),
            Reader::Log => Some(Listener::follow(tuning.log_file.clone())),
        };
        let shared = Arc::new(Mutex::new(Shared {
            state,
            rest,
            resend: false,
            tuning,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            thread::spawn(move || run(&shared, &stop))
        };
        Live {
            reader,
            _listener: listener,
            shared,
            stop,
            thread: Some(thread),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn set(&mut self, state: State) {
        self.lock().state = state;
    }
    /// A new look, from the file; the frame thread takes it on its next tick.
    pub fn tune(&mut self, tuning: Tuning) {
        self.lock().tuning = tuning;
    }
    pub fn reader(&self) -> Reader {
        self.reader
    }
    /// A new resting colour (a profile switch); the switch wrote the backlight, so the
    /// current colour goes up again over it.
    pub fn rest(&mut self, rest: [u8; 3]) {
        let mut s = self.lock();
        s.rest = rest;
        s.resend = true;
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(shared: &Mutex<Shared>, stop: &AtomicBool) {
    // The watcher joins the previous Live thread before starting another. Keep the
    // attack clock across those threads so leaving/re-entering a game profile
    // cannot bypass the six-second limit. Preview/test meters keep their own clocks.
    static LAST_ATTACK_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    static LAST_FORTRESS_FLASH: Mutex<Option<Instant>> = Mutex::new(None);
    let mut meter = Meter {
        attack_flash: *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()),
        ..Meter::default()
    };
    meter.fortress.last_flash = *LAST_FORTRESS_FLASH
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut t0 = Instant::now();
    let mut n = 0u32;
    // The colour on the device: the profile's until the meter writes one. Waiting alone
    // never touches the backlight.
    let mut sent: Option<[u8; 3]> = None;
    let mut last_error = String::new();
    while !stop.load(Ordering::Relaxed) {
        if crate::lcd::editor_open() {
            thread::sleep(Duration::from_millis(250));
            continue;
        }
        let (state, resend, rest) = {
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            let resend = std::mem::take(&mut s.resend);
            sent.get_or_insert(s.rest);
            if s.tuning != meter.tuning {
                meter.tuning = s.tuning.clone();
            }
            (s.state, resend, s.rest)
        };
        let frame = meter.frame(state);
        let want = meter.colour(state, rest);
        let colour = if sent != Some(want) || resend {
            let [r, g, b] = want;
            crate::daemon::send(&format!("rgb {r} {g} {b}\n")).map(|_| sent = Some(want))
        } else {
            Ok(())
        };
        if let Err(e) = colour.and_then(|_| crate::daemon::send_lcd(&frame.0)) {
            // The daemon is gone or not reading; say so once, try again in a while.
            if e != last_error {
                eprintln!("health meter: {e}");
                last_error = e;
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        n += 1;
        let due = t0 + TICK * n;
        let now = Instant::now();
        if due > now {
            thread::sleep(due - now);
        } else {
            // Fell behind (a slow pipe); the clock starts again from here.
            t0 = now;
            n = 0;
        }
    }
    *LAST_ATTACK_FLASH.lock().unwrap_or_else(|e| e.into_inner()) = meter.attack_flash;
    *LAST_FORTRESS_FLASH
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = meter.fortress.last_flash;
    // The resting colour as it is now: the profile that ends the meter set it just
    // before stopping us (found 2026-10-07: the old profile's olive came back over
    // the new one's purple).
    let rest = shared.lock().unwrap_or_else(|e| e.into_inner()).rest;
    if sent.is_some_and(|c| c != rest) {
        let [r, g, b] = rest;
        if let Err(e) = crate::daemon::send(&format!("rgb {r} {g} {b}\n")) {
            eprintln!("health meter colour: {e}");
        }
    }
}

// ---- feeders ----

/// A scripted pass through every state, for a look at the meter without a game.
pub fn demo() -> Result<String, String> {
    let active = crate::active_name();
    // Health mode is the profile's own line since 0.2.3x; an old picture name still counts.
    let shows = crate::load(&active)
        .ok()
        .is_some_and(|(p, _)| p.health.is_some() || selects(p.lcd.as_deref()).is_some());
    if !shows {
        println!(
            "note: profile '{active}' has no health mode (g13map profile health '{active}' \
             feed); the panel will not follow this run"
        );
    }
    let steps: &[(&str, Option<State>, u64)] = &[
        ("connected, waiting", Some(State::Wait), 5000),
        (
            "full health, half shield",
            Some(State::Health {
                pct: 100,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            }),
            3000,
        ),
        (
            "90",
            Some(State::Health {
                pct: 90,
                shield: 20,
                helmet: false,
                extra: Extra::NONE,
            }),
            1500,
        ),
        ("76, shield gone", Some(State::health(76)), 2000),
        ("75: yellow", Some(State::health(75)), 2000),
        ("60", Some(State::health(60)), 1500),
        ("50: orange", Some(State::health(50)), 2000),
        ("35", Some(State::health(35)), 1500),
        ("25: red, the alarm", Some(State::health(25)), 4000),
        ("10", Some(State::health(10)), 4000),
        (
            "0: flatline, lights out; then the search under red",
            Some(State::health(0)),
            8000,
        ),
        ("back to 100", Some(State::health(100)), 2000),
        (
            "125: overshield",
            Some(State::Health {
                pct: 125,
                shield: 100,
                helmet: false,
                extra: Extra::NONE,
            }),
            4000,
        ),
        (
            "150, armour 200",
            Some(State::Health {
                pct: 150,
                shield: 200,
                helmet: false,
                extra: Extra::NONE,
            }),
            2000,
        ),
        ("game gone", None, 0),
    ];
    for (what, state, ms) in steps {
        println!("{what}");
        write(*state, None)?;
        thread::sleep(Duration::from_millis(*ms));
    }
    Ok("demo done; the profile's picture and colour are back".into())
}

/// How long a Counter-Strike 2 line lives; the game posts a heartbeat every 5 s.
const CS2_TTL: Duration = Duration::from_secs(30);
pub const CS2_PORT: u16 = 3000;

/// The Game State Integration file for `g13map health cs2`: goes into the game's
/// `game/csgo/cfg/` as `gamestate_integration_g13map.cfg`.
pub fn cs2_config(port: u16) -> String {
    format!(
        "\"g13map health\"
{{
  \"uri\" \"http://127.0.0.1:{port}\"
  \"timeout\" \"5.0\"
  \"buffer\" \"0.1\"
  \"throttle\" \"0.1\"
  \"heartbeat\" \"5.0\"
  \"data\"
  {{
    \"provider\" \"1\"
    \"map\" \"1\"
    \"player_id\" \"1\"
    \"player_state\" \"1\"
  }}
}}
"
    )
}

/// What a Counter-Strike 2 game-state post says: the player's health and armour while
/// `player` is the one at the keyboard. Dead, `player` is whoever is being spectated:
/// waiting (asked 2026-10-07; the listener puts a drop to zero in front of it). The
/// menu and the lobby read as full health and no armour (for looks), and a post that is
/// not even JSON as waiting.
pub fn cs2_state(body: &str) -> State {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return State::Wait,
    };
    let me = v["provider"]["steamid"].as_str();
    let player = &v["player"];
    if me.is_some() && player["steamid"].as_str() != me {
        return State::Wait;
    }
    match player["state"]["health"].as_u64() {
        Some(h) => State::Health {
            pct: h.min(999) as u32,
            shield: player["state"]["armor"].as_u64().unwrap_or(0).min(100) as u32,
            helmet: player["state"]["helmet"].as_bool().unwrap_or(false),
            extra: Extra::NONE,
        },
        None => State::health(100),
    }
}

/// Reads one HTTP request and returns its body (up to 1 MB).
fn request_body(r: &mut impl Read) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return Err("request header too long".into());
        }
        match r.read(&mut chunk) {
            Ok(0) => return Err("connection closed before the header ended".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e.to_string()),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    if length > 1024 * 1024 {
        return Err("request body too long".into());
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        match r.read(&mut chunk) {
            Ok(0) => return Err("connection closed before the body ended".into()),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e.to_string()),
        }
    }
    body.truncate(length);
    Ok(String::from_utf8_lossy(&body).to_string())
}

fn serve(mut s: TcpStream) -> Result<State, String> {
    s.set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let body = request_body(&mut s)?;
    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    Ok(cs2_state(&body))
}

/// `g13map health cs2`: takes the game's posts on the loopback port and feeds the meter
/// until stopped. The feed carries a TTL, so a closed game or a stopped listener is a
/// meter gone within the TTL.
pub fn cs2(port: u16) -> Result<String, String> {
    cs2_serve(port, &AtomicBool::new(false))
}

/// A reader on a thread of its own: the CS2 listener for `health cs2`, the log
/// follower for `health log`; dropping it stops the thread (and frees the port).
pub struct Listener {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Listener {
    /// Follows the game's console log into the feed.
    pub fn follow(path: PathBuf) -> Listener {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || follow_log(&path, &stop))
        };
        Listener {
            stop,
            thread: Some(thread),
        }
    }
    pub fn start(port: u16) -> Listener {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || {
                if let Err(e) = cs2_serve(port, &stop) {
                    eprintln!("health meter: {e}");
                }
            })
        };
        Listener {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Takes the game's posts until `stop` is set.
fn cs2_serve(port: u16, stop: &AtomicBool) -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    eprintln!(
        "g13map health cs2: listening on http://127.0.0.1:{port}; the game needs \
         gamestate_integration_g13map.cfg (g13map health cs2-config {port}) in game/csgo/cfg/"
    );
    let mut last: Option<State> = None;
    while !stop.load(Ordering::Relaxed) {
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => {
                eprintln!("g13map health cs2: {e}");
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        let state = match stream
            .set_nonblocking(false)
            .map_err(|e| e.to_string())
            .and_then(|_| serve(stream))
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("g13map health cs2: {e}");
                continue;
            }
        };
        if last != Some(state) {
            eprintln!("g13map health cs2: {state:?}");
            // Alive one post, spectating the next: a death the game did not spell out
            // as zero health. The drop goes first, long enough for the watcher to see it;
            // the meter then holds its dark seconds before showing the wait.
            if state == State::Wait && matches!(last, Some(State::Health { pct, .. }) if pct > 0) {
                write(Some(State::health(0)), Some(CS2_TTL))?;
                thread::sleep(TICK * 3);
            }
            last = Some(state);
        }
        write(Some(state), Some(CS2_TTL))?;
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;

    #[test]
    fn bands_follow_the_asked_edges() {
        let d = Tuning::default();
        let edges = [
            (0, "dead"),
            (1, "red"),
            (25, "red"),
            (26, "orange"),
            (50, "orange"),
            (51, "yellow"),
            (75, "yellow"),
            (76, "green"),
            (100, "green"),
            (101, "blue"),
            (999, "blue"),
        ];
        for (pct, band) in edges {
            assert_eq!(d.band_at(pct).name, band, "{pct}");
        }
        assert_eq!(d.alarm().name, "red");
        assert_eq!(d.notches(), vec![42, 80, 118]);
        // A file's own ladder replaces the default one, in any order; death stays, the
        // lowest band above it is the alarm, and a band past 200 is just another line.
        let t = Tuning::parse(
            "band purple 201 160 0 255\nband low 1 9 9 9\nband high 101 0 0 9\n\
             band mid 40 1 2 3\nband bad 1000 1 1 1\nband bad2 10 1 2 999\nband short 5 1\n",
        );
        let names: Vec<(u32, &str)> = t.bands.iter().map(|b| (b.from, b.name.as_str())).collect();
        assert_eq!(
            names,
            vec![
                (0, "dead"),
                (1, "low"),
                (40, "mid"),
                (101, "high"),
                (201, "purple")
            ]
        );
        assert_eq!(t.rgb_at(0), [0, 0, 0]);
        assert_eq!(t.rgb_at(39), [9, 9, 9]);
        assert_eq!(t.rgb_at(100), [1, 2, 3]);
        assert_eq!(t.rgb_at(250), [160, 0, 255]);
        assert!(t.is_alarm(39) && !t.is_alarm(40) && !t.is_alarm(0));
        assert_eq!(t.notches(), vec![BAR_X + BAR_W * 39 / 100]);
        // Redefining death keeps it at the bottom; an old-style colour line recolours.
        let t = Tuning::parse("band dead 0 1 1 1\nband one 1 2 2 2\nred 7 7 7\none 8 8 8\n");
        assert_eq!(t.bands.len(), 2);
        assert_eq!((t.rgb_at(0), t.rgb_at(50)), ([1, 1, 1], [8, 8, 8]));
    }

    #[test]
    fn tuning_file_round_trips_and_tolerates_junk() {
        let d = Tuning::default();
        assert_eq!(Tuning::parse(&d.to_text()), d);
        assert_eq!(Tuning::parse(""), d);
        let t = Tuning::parse(
            "yellow 255 255 0 # more yellow\nhold 1.5\nflash off\ncalm abc\nracing 0\nswell 0.25\nnonsense 1 2 3\nred 1 2\nheart 200 -5\nreadout 10 2\n",
        );
        assert_eq!(t.rgb_at(60), [255, 255, 0]);
        assert_eq!(t.rgb_at(10), d.rgb_at(10));
        assert_eq!(t.hold, 1.5);
        assert_eq!(t.flash, Flash::Off);
        assert_eq!((t.heart, t.readout), ([144, 0], [34, 2]));
        assert_eq!(
            Tuning::parse("flash trace\nheart 20 10\n").flash,
            Flash::Trace
        );
        assert_eq!((t.swell, t.swell_ticks()), (0.25, 3));
        assert_eq!((t.calm, t.racing), (d.calm, d.racing));
        // The hold and the beat spacing follow the file.
        let mut m = Meter {
            tuning: t.clone(),
            ..Default::default()
        };
        for _ in 0..15 {
            m.frame(State::health(0));
        }
        assert!(m.holding());
        m.frame(State::health(0));
        assert!(!m.holding());
        assert_eq!(d.period(100), 100);
        assert_eq!(d.period(0), 40);
        assert_eq!(
            Tuning {
                calm: 0.5,
                ..d.clone()
            }
            .period(100),
            ECG_COLUMNS
        );
    }

    #[test]
    fn factorio_attack_flash_has_a_wall_clock_cooldown() {
        for letter in 'A'..='Z' {
            assert_ne!(glyph(letter), &GLYPHS[10], "missing LCD letter {letter}");
        }
        let written = SystemTime::now();
        let state = |count| {
            parse(&format!("150/250 shield 30/150 battery 25 research 37 technology military attack {count} ttl 3"), written, written).unwrap()
        };
        let start = Instant::now();
        let mut m = Meter::default();
        assert_eq!(m.frame_at(state(2), start).lit(), W * H);
        assert_eq!(
            m.frame_at(state(2), start + Duration::from_millis(299))
                .lit(),
            W * H
        );
        assert!(
            m.frame_at(state(2), start + Duration::from_millis(300))
                .lit()
                < W * H / 2
        );
        m.frame_at(state(0), start + Duration::from_secs(1));
        // Clearing/reappearing or a changed count must not reset the cooldown.
        assert!(m.frame_at(state(8), start + Duration::from_secs(2)).lit() < W * H / 2);
        assert!(
            m.frame_at(state(9), start + Duration::from_millis(5999))
                .lit()
                < W * H / 2
        );
        assert_eq!(
            m.frame_at(state(9), start + Duration::from_secs(6)).lit(),
            W * H
        );
        // A replacement Live renderer inherits this timestamp on a profile switch.
        let mut restarted = Meter {
            attack_flash: m.attack_flash,
            ..Meter::default()
        };
        assert!(
            restarted
                .frame_at(state(2), start + Duration::from_secs(7))
                .lit()
                < W * H / 2
        );
        assert_eq!(
            restarted
                .frame_at(state(2), start + Duration::from_secs(12))
                .lit(),
            W * H
        );
        assert!(
            m.frame_at(State::Wait, start + Duration::from_millis(6100))
                .lit()
                < W * H / 2
        );
        assert_eq!(
            parse(
                "100 attack 2 ttl 3",
                written,
                written + Duration::from_secs(4)
            ),
            None
        );
        for line in ["100 attack -1", "wait attack 1", "100 technology bad/name"] {
            assert!(parse(line, written, written).is_none(), "{line}");
        }
        assert!(parse(
            &format!("100 technology {}", "x".repeat(257)),
            written,
            written
        )
        .is_none());
    }

    #[test]
    fn vehicle_gears_crack_at_orange_and_recover_above_it() {
        let mut tuning = Tuning::default();
        assert_eq!(tuning.vehicle, VehicleStyle::Gear);
        assert!(!tuning.vehicle_damaged(51));
        for pct in [50, 26, 25, 0] {
            assert!(tuning.vehicle_damaged(pct));
        }
        tuning
            .bands
            .iter_mut()
            .find(|b| b.name == "yellow")
            .unwrap()
            .from = 71;
        assert!(tuning.vehicle_damaged(70));
        assert!(!tuning.vehicle_damaged(71));
        tuning.bands.retain(|b| b.from <= 26);
        assert!(tuning.vehicle_damaged(100)); // orange is the highest band
        tuning.bands.retain(|b| b.name != "orange");
        assert!(tuning.vehicle_damaged(50));
        assert!(!tuning.vehicle_damaged(51)); // no named orange uses the fallback
        let now = SystemTime::now();
        let healthy = parse("51 vehicle car pilot 80", now, now).unwrap();
        let damaged = parse("50 vehicle car pilot 80", now, now).unwrap();
        let mut m = Meter::default();
        let first = m.frame(healthy);
        let later = m.frame(healthy);
        assert!((0..19).any(|y| (0..33).any(|x| first.get(x, y) != later.get(x, y))));
        for tick in 0..48 {
            m.tick = tick;
            let intact = m.frame(healthy);
            m.tick = tick;
            let cracked = m.frame(damaged);
            let mut removed = 0;
            for y in 0..19 {
                for x in 0..33 {
                    assert!(!cracked.get(x, y) || intact.get(x, y));
                    removed += usize::from(intact.get(x, y) && !cracked.get(x, y));
                }
            }
            assert!(removed >= 18, "cracks remain visible at phase {tick}");
            m.tick = tick;
            let recovered = m.frame(healthy);
            assert_eq!(intact, recovered);
        }
    }

    #[test]
    fn vehicle_render_does_not_beat_or_latch_vehicle_zero() {
        let now = SystemTime::now();
        let state = parse("20 vehicle tank pilot 60 shield 20 battery 100", now, now).unwrap();
        let State::Health { pct, extra, .. } = state else {
            panic!("expected vehicle")
        };
        assert_eq!(pct, 20);
        assert_eq!(extra.pilot, Some(60));
        for style in [VehicleStyle::Tread, VehicleStyle::Gear, VehicleStyle::Scan] {
            let mut m = Meter::default();
            m.tuning.vehicle = style;
            m.tick = i32::MAX as u32; // animations remain safe across the signed boundary
            let first = m.frame(state);
            for _ in 0..48 {
                let frame = m.frame(state);
                for y in 2..19 {
                    for x in 33..W {
                        assert_eq!(first.get(x, y), frame.get(x, y), "vitals stay steady");
                    }
                }
                assert!(m.lift.iter().all(|v| *v == 0));
                assert_eq!(m.beat_age, None);
            }
            m.frame(parse("0 vehicle tank pilot 60", now, now).unwrap());
            assert_eq!(m.dead, 0);
            m.frame(State::health(60));
            assert!(!m.holding());
        }
        for line in ["wait vehicle tank", "100 pilot NaN", "100 vehicle bad/name"] {
            assert!(parse(line, now, now).is_none());
        }
    }

    #[test]
    fn research_marquee_clips_name_and_keeps_percentage_fixed() {
        let label = Label::parse("advanced_uranium_fuel_processing").unwrap();
        let mut m = Meter::default();
        let mut first = Bitmap::blank();
        m.research(&mut first, 43, 19, label, 37);
        let mut paused = Bitmap::blank();
        m.tick = 12;
        m.research(&mut paused, 43, 19, label, 37);
        assert_eq!(first, paused);
        let mut later = Bitmap::blank();
        m.tick = 24;
        m.research(&mut later, 43, 19, label, 37);
        assert_ne!(first, later);
        for y in 0..H {
            for x in 0..W {
                if !(43..96).contains(&x) {
                    assert_eq!(first.get(x, y), later.get(x, y), "only the name scrolls");
                }
            }
        }
        let mut changed = Bitmap::blank();
        m.research(
            &mut changed,
            43,
            19,
            Label::parse("advanced_solar_power").unwrap(),
            37,
        );
        assert_eq!(m.research_started, 24);
        assert_eq!(Label::parse(&"x".repeat(256)).unwrap().as_str().len(), 256);
        assert!(Label::parse(&"x".repeat(257)).is_none());
    }

    #[test]
    fn terraria_resources_parse_render_and_expire() {
        let now = SystemTime::now();
        let line = "240/400 mana 80/200 defense 45 breath 80/200 ttl 3";
        let state = parse(line, now, now).unwrap();
        let State::Health {
            pct, extra, shield, ..
        } = state
        else {
            panic!("expected health");
        };
        assert_eq!((pct, shield), (60, 0));
        assert_eq!(
            extra.mana,
            Some(Resource {
                current: 80,
                maximum: 200
            })
        );
        assert_eq!(extra.defense, Some(45));
        assert_eq!(extra.breath.unwrap().percent(), 40);
        assert_eq!(parse(line, now, now + Duration::from_secs(4)), None);
        for invalid in [
            "100 mana -1/200",
            "100 mana 2/0",
            "100 mana 20",
            "wait mana 0/0",
            "100 breath 1/NaN",
            "100 defense -4",
            "100 ttl inf",
            "100 ttl NaN",
            "100 ttl 1e300",
        ] {
            assert_eq!(parse(invalid, now, now), None, "{invalid}");
        }
        assert!(parse("100 mana 0/0", now, now).is_some());
        let frame = Meter::default().frame(state);
        let no_mana = parse("240/400 mana 0/200 defense 45 breath 80/200", now, now).unwrap();
        let empty_frame = Meter::default().frame(no_mana);
        assert!(
            lit_in(&frame, 45, 13, 106, 15) > lit_in(&empty_frame, 45, 13, 106, 15),
            "mana gauge fills"
        );
        assert!(lit_in(&frame, 43, 2, 108, 9) > 30, "mana label and values");
        assert!(lit_in(&frame, 43, 19, 84, 26) > 20, "defense");
        let full_air = parse("240/400 mana 80/200 defense 45 breath 200/200", now, now).unwrap();
        let full_frame = Meter::default().frame(full_air);
        assert!(
            lit_in(&frame, 88, 19, 138, 26) > lit_in(&full_frame, 88, 19, 138, 26),
            "low air appears"
        );
    }

    #[test]
    fn feed_lines_parse_and_expire() {
        let now = SystemTime::now();
        let p = |t: &str| parse(t, now, now);
        assert_eq!(p("87"), Some(State::health(87)));
        assert_eq!(p("50/200\n"), Some(State::health(25)));
        assert_eq!(p("87.5/100"), Some(State::health(88)));
        assert_eq!(p("120"), Some(State::health(120)));
        assert_eq!(p("5000"), Some(State::health(999)));
        assert_eq!(
            p("80 shield 25/50"),
            Some(State::Health {
                pct: 80,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        assert_eq!(
            p("80 shield 50 helmet on"),
            Some(State::Health {
                pct: 80,
                shield: 50,
                helmet: true,
                extra: Extra::NONE,
            })
        );
        assert_eq!(
            p("80 shield 50 helmet off"),
            Some(State::Health {
                pct: 80,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        assert_eq!(p("80 helmet yes"), None);
        assert_eq!(p("wait helmet on"), None);
        // A game's extras, each its own word; the line written back reads the same.
        let v1 = p("72/100 cap 85 rank SS style 40 time 11561.7 dash 2.5 rail 80 ttl 3");
        assert_eq!(
            v1,
            Some(State::Health {
                pct: 72,
                shield: 0,
                helmet: false,
                extra: Extra {
                    cap: Some(85),
                    rank: Some(5),
                    style: Some(40),
                    time: Some(11561),
                    dash: Some(25),
                    rail: Some(80),
                    ..Extra::NONE
                },
            })
        );
        assert_eq!(p("72 rank X"), None);
        assert_eq!(p("wait rank S"), None);
        assert_eq!(p("72 dash 9"), p("72 dash 3"));
        assert_eq!(clock(11561), "3:12:41");
        assert_eq!(clock(161), "2:41");
        assert_eq!(clock(0), "0:00");
        assert_eq!(p("wait"), Some(State::Wait));
        assert_eq!(p("wait ttl 30"), Some(State::Wait));
        for bad in [
            "",
            "off",
            "abc",
            "0/0",
            "-5",
            "nan",
            "wait shield 5",
            "99 ttl",
            "99 ttl x",
            "99 ttl 0",
            "99 x 1",
        ] {
            assert_eq!(p(bad), None, "{bad:?}");
        }
        let old = now - Duration::from_secs(31);
        assert_eq!(parse("99 ttl 30", old, now), None);
        assert_eq!(
            parse("99 ttl 30", now - Duration::from_secs(29), now),
            Some(State::health(99))
        );
        assert_eq!(parse("99", old, now), Some(State::health(99)));
    }

    #[test]
    fn feed_file_round_trip() {
        let _box = Sandbox::new("meter");
        assert_eq!(read(), None);
        let now = SystemTime::now();
        let vehicle = parse("60 vehicle tank pilot 150/250 shield 20 battery 25 research 37 technology advanced-oil-processing", now, now).unwrap();
        write(Some(vehicle), Some(Duration::from_secs(30))).unwrap();
        assert_eq!(read(), Some(vehicle));
        write(None, None).unwrap();
        write(
            Some(State::Health {
                pct: 42,
                shield: 7,
                helmet: false,
                extra: Extra::NONE,
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            read(),
            Some(State::Health {
                pct: 42,
                shield: 7,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        write(Some(State::Wait), Some(Duration::from_secs(30))).unwrap();
        assert_eq!(read(), Some(State::Wait));
        assert!(fs::read_to_string(path().unwrap())
            .unwrap()
            .ends_with("ttl 30\n"));
        write(None, None).unwrap();
        assert_eq!(read(), None);
        write(None, None).unwrap();
        // A feeder in a container writes the state-directory file; the newest file wins.
        prepare();
        let state = crate::state_dir().join("health");
        fs::write(&state, "60 shield 10 ttl 30\n").unwrap();
        assert_eq!(
            read(),
            Some(State::Health {
                pct: 60,
                shield: 10,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        thread::sleep(Duration::from_millis(20));
        write(Some(State::health(70)), None).unwrap();
        assert_eq!(read(), Some(State::health(70)));
        write(None, None).unwrap();
        assert!(!state.exists());
        assert_eq!(read(), None);
    }

    /// V1 at `pct` with every extra word a game can send.
    pub(super) fn v1_state(pct: u32) -> State {
        State::Health {
            pct,
            shield: 0,
            helmet: false,
            extra: Extra {
                cap: Some(85),
                rank: Some(5),
                style: Some(40),
                time: Some(11561),
                dash: Some(25),
                rail: Some(80),
                ..Extra::NONE
            },
        }
    }

    fn lit_in(bm: &Bitmap, x0: usize, y0: usize, x1: usize, y1: usize) -> usize {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| bm.get(x, y))
            .count()
    }

    #[test]
    fn every_state_draws_and_moves() {
        for state in [
            State::Wait,
            State::health(0),
            State::health(1),
            State::health(25),
            State::health(50),
            State::health(75),
            State::health(100),
            State::Health {
                pct: 101,
                shield: 100,
                helmet: false,
                extra: Extra::NONE,
            },
            State::health(150),
            State::health(999),
        ] {
            let mut m = Meter::default();
            let frames: Vec<Bitmap> = (0..80).map(|_| m.frame(state)).collect();
            assert!(frames.iter().all(|f| f.lit() > 0), "{state:?}");
            assert!(
                frames.windows(2).any(|w| w[0].0 != w[1].0),
                "{state:?} is still"
            );
            // The readout sits top right, the bar box along the bottom.
            assert!(
                lit_in(&frames[0], 120, 2, 158, 16) > 20,
                "{state:?} readout"
            );
            assert_eq!(lit_in(&frames[0], 2, 42, 158, 43), 156 - 3, "{state:?} bar");
        }
        // The bar fills with health, the shield bar only with shield.
        let bar = |s: State| lit_in(&Meter::default().frame(s), 4, 36, 156, 41);
        assert_eq!(bar(State::health(0)), 0);
        assert_eq!(bar(State::health(50)), 76 * 5);
        assert_eq!(bar(State::health(100)), 152 * 5);
        assert_eq!(bar(State::health(150)), 152 * 5);
        let shield = |s: State| lit_in(&Meter::default().frame(s), 4, 30, 156, 33);
        assert_eq!(shield(State::health(100)), 0);
        assert!(
            shield(State::Health {
                pct: 100,
                shield: 100,
                helmet: false,
                extra: Extra::NONE,
            }) > 200
        );
        // A helmet makes the shield bar solid: every pixel of its three rows.
        assert_eq!(
            shield(State::Health {
                pct: 100,
                shield: 100,
                helmet: true,
                extra: Extra::NONE,
            }),
            152 * 3
        );
        // Past 100 the second hundred adds a row above and below, from the left, and a
        // ring around the heart per hundred, up to three (Doom's blue armour is 200).
        let armour = |s: u32| State::Health {
            pct: 100,
            shield: s,
            helmet: true,
            extra: Extra::NONE,
        };
        let rim = |s: State| {
            let f = Meter::default().frame(s);
            (lit_in(&f, 4, 29, 156, 30), lit_in(&f, 4, 33, 156, 34))
        };
        assert_eq!(rim(armour(100)), (0, 0));
        assert_eq!(rim(armour(150)), (76, 76));
        assert_eq!(rim(armour(200)), (152, 152));
        assert_eq!(rim(armour(999)), (152, 152));
        let [hx, hy] = Tuning::default().heart.map(|v| v as usize);
        let outer = |s: State| {
            let f = Meter::default().frame(s);
            lit_in(&f, hx - 5, hy - 4, hx + 12, hy - 3)
                + lit_in(&f, hx - 5, hy + 9, hx + 12, hy + 10)
        };
        assert_eq!(outer(armour(100)), 0);
        assert_eq!(outer(armour(101)), 2 * 15);
        let third = |s: State| {
            let f = Meter::default().frame(s);
            lit_in(&f, hx - 6, hy - 5, hx + 13, hy - 4)
                + lit_in(&f, hx - 6, hy + 10, hx + 13, hy + 11)
        };
        assert_eq!(third(armour(200)), 0);
        assert_eq!(third(armour(201)), 2 * 17);
        assert_eq!(third(armour(999)), 2 * 17);
        // A flatline never beats and holds still, dark, for three seconds; then the
        // search for a pulse begins under red, and any health ends the count.
        let mut m = Meter::default();
        // A game's extras draw their own elements: the rank at the top centre with its
        // meter, the timer under the readout, the pips and the charge under the sprite,
        // the hard-damage hatch at the end of the bar; none of them without the words.
        let plain = Meter::default().frame(State::health(72));
        let v1 = Meter::default().frame(v1_state(72));
        assert_eq!(lit_in(&plain, 64, 2, 100, 21), 0);
        assert!(lit_in(&v1, 64, 2, 100, 16) > 20, "rank letters");
        assert!(lit_in(&v1, 64, 17, 100, 22) > 10, "style meter");
        assert!(lit_in(&v1, 120, 18, 158, 25) > 15, "timer");
        assert_eq!(lit_in(&plain, 4, 36, 156, 41), 109 * 5);
        assert!(
            lit_in(&v1, 4, 36, 156, 41) > 109 * 5 + 40,
            "hard-damage hatch"
        );
        assert!(lit_in(&v1, 3, 13, 42, 19) > 10, "dash pips and the charge");
        // V1 in the heart's place, from a profile's own lines over the file.
        let mut v = Meter {
            tuning: Tuning::parse(&format!(
                "{}\nsprite v1\nheart 2 1\n",
                Tuning::default().to_text()
            )),
            ..Default::default()
        };
        assert_eq!((v.tuning.sprite, v.tuning.heart), (Sprite::V1, [2, 1]));
        let f = v.frame(v1_state(72));
        assert!(lit_in(&f, 2, 1, 17, 14) > 40, "V1 is drawn");
        let dead: Vec<Bitmap> = (0..40).map(|_| m.frame(State::health(0))).collect();
        assert!(m.beat_age.is_none());
        assert!(dead[..30].windows(2).all(|w| w[0].0 == w[1].0));
        assert!(dead[31..].windows(2).any(|w| w[0].0 != w[1].0));
        let mut m = Meter::default();
        let colours: Vec<[u8; 3]> = (0..32)
            .map(|_| {
                m.frame(State::health(0));
                m.colour(State::health(0), [9, 9, 9])
            })
            .collect();
        assert_eq!(colours[29], [0, 0, 0]);
        assert_eq!(colours[31], Tuning::default().rgb_at(1));
        m.frame(State::health(5));
        assert_eq!(m.dead, 0);
        m.frame(State::Wait);
        assert_eq!(m.colour(State::Wait, [9, 9, 9]), [9, 9, 9]);
        // One zero latches the whole hold: the feed may already say 100 or wait, the
        // panel stays dark and flat for thirty ticks, then shows what the feed says.
        for next in [State::health(100), State::Wait] {
            let mut m = Meter::default();
            let first = m.frame(State::health(0));
            assert_eq!(m.colour(State::health(0), [9, 9, 9]), [0, 0, 0]);
            for i in 1..30 {
                assert_eq!(m.frame(next).0, first.0, "{next:?} tick {i}");
                assert_eq!(m.colour(next, [9, 9, 9]), [0, 0, 0], "{next:?} tick {i}");
            }
            let after = m.frame(next);
            assert_ne!(after.0, first.0, "{next:?} after the hold");
            let want = next
                .pct()
                .map_or([9, 9, 9], |p| Tuning::default().rgb_at(p));
            assert_eq!(m.colour(next, [9, 9, 9]), want, "{next:?} colour after");
        }
        // Full health beats every 100 columns, 20 ticks, the first on the fifth tick, as
        // the R peak enters (indices are zero-based).
        let mut m = Meter::default();
        let beats: Vec<u32> = (0..200u32)
            .filter(|_| {
                m.frame(State::health(100));
                m.beat_age == Some(0)
            })
            .collect();
        assert_eq!(beats, (0..10).map(|i| 4 + 20 * i).collect::<Vec<_>>());
        // On the last quarter the beat frame is the panel inverted: mostly lit.
        let mut m = Meter::default();
        let flash = (0..40)
            .map(|_| m.frame(State::health(10)))
            .max_by_key(|f| f.lit())
            .unwrap();
        assert!(flash.lit() > W * H / 2);
    }

    #[test]
    fn profile_names_select_the_meter() {
        assert_eq!(selects(Some("health")), Some(Reader::Feed));
        assert_eq!(selects(Some("health cs2")), Some(Reader::Cs2));
        assert_eq!(selects(Some("healthy")), None);
        assert_eq!(selects(None), None);
        assert!(preview("health").is_some_and(|bm| bm.lit() > 100));
        assert!(preview("aquarium").is_none());
        assert_eq!(Tuning::parse("cs2_port 3100").cs2_port, 3100);
        assert_eq!(Tuning::parse("cs2_port x").cs2_port, CS2_PORT);
        // The listener binds, stops and frees its port.
        let port = 3900 + (std::process::id() % 100) as u16;
        let l = Listener::start(port);
        thread::sleep(Duration::from_millis(100));
        assert!(TcpListener::bind(("127.0.0.1", port)).is_err());
        drop(l);
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn cs2_posts_become_states() {
        let me = r#""provider":{"steamid":"1"}"#;
        let post = |player: &str| cs2_state(&format!("{{{me},\"player\":{{{player}}}}}"));
        assert_eq!(
            post(r#""steamid":"1","state":{"health":87,"armor":50}"#),
            State::Health {
                pct: 87,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            }
        );
        assert_eq!(
            post(r#""steamid":"1","state":{"health":0}"#),
            State::health(0)
        );
        assert_eq!(
            post(r#""steamid":"1","activity":"menu""#),
            State::health(100)
        );
        assert_eq!(post(r#""steamid":"2","state":{"health":87}"#), State::Wait);
        assert_eq!(
            post(r#""steamid":"1","state":{"health":50,"armor":90,"helmet":true}"#),
            State::Health {
                pct: 50,
                shield: 90,
                helmet: true,
                extra: Extra::NONE,
            }
        );
        assert_eq!(cs2_state("{}"), State::health(100));
        assert_eq!(cs2_state("not json"), State::Wait);
        assert!(cs2_config(3000).contains("http://127.0.0.1:3000"));
    }

    #[test]
    fn http_bodies_are_read_whole() {
        let body = r#"{"a":1}"#;
        let req = format!(
            "POST / HTTP/1.1\r\nHost: x\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(request_body(&mut req.as_bytes()).unwrap(), body);
        assert_eq!(
            request_body(&mut &b"GET / HTTP/1.1\r\n\r\n"[..]).unwrap(),
            ""
        );
        assert!(request_body(&mut &b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nab"[..]).is_err());
        assert!(request_body(&mut &b"POST / HTTP/1.1\r\n"[..]).is_err());
    }
}

/// `G13MAP_METER_DUMP=DIR cargo test --lib meter::dump -- --ignored`: forty frames of
/// every state as PBM files under DIR, for a contact sheet, and the demo as
/// `demo-NNN.pbm` with `demo.colours` (one `R G B` backlight per frame) for
/// `tools/health-gif.sh`, which renders the README's GIF from them.
#[cfg(test)]
mod dump {
    use super::*;
    fn pbm(dir: &std::path::Path, name: &str, bm: &Bitmap) {
        let mut bytes = format!("P4\n{W} {H}\n").into_bytes();
        for y in 0..H {
            for xb in 0..W.div_ceil(8) {
                let mut b = 0u8;
                for bit in 0..8 {
                    let x = xb * 8 + bit;
                    if x < W && !bm.get(x, y) {
                        b |= 0x80 >> bit;
                    }
                }
                bytes.push(b);
            }
        }
        fs::write(dir.join(name), bytes).unwrap();
    }
    #[test]
    #[ignore]
    fn vehicles() {
        let Some(dir) = std::env::var_os("G13MAP_METER_DUMP") else {
            return;
        };
        for (tag, style, pct) in [
            ("tread", VehicleStyle::Tread, 60),
            ("gear", VehicleStyle::Gear, 60),
            ("scan", VehicleStyle::Scan, 60),
            ("gear-cracked", VehicleStyle::Gear, 40),
        ] {
            let state = parse(&format!("{pct} vehicle tank pilot 80 shield 20 battery 25 research 37 technology military attack 0"), SystemTime::now(), SystemTime::now()).unwrap();
            let dir = PathBuf::from(&dir).join(tag);
            fs::create_dir_all(&dir).unwrap();
            let mut m = Meter::default();
            m.tuning.vehicle = style;
            let mut colours = String::new();
            for i in 0..48 {
                let bm = m.frame(state);
                pbm(&dir, &format!("demo-{i:03}.pbm"), &bm);
                let [r, g, b] = m.colour(state, [0, 0, 0]);
                colours.push_str(&format!("{r} {g} {b}\n"));
            }
            fs::write(dir.join("demo.colours"), colours).unwrap();
        }
    }
    #[test]
    #[ignore]
    fn frames() {
        let Some(dir) = std::env::var_os("G13MAP_METER_DUMP") else {
            return;
        };
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).unwrap();
        let full = |pct, shield, helmet| State::Health {
            pct,
            shield,
            helmet,
            extra: Extra::NONE,
        };
        for (tag, state) in [
            ("wait", State::Wait),
            ("h000", State::health(0)),
            ("h010", State::health(10)),
            ("h025", State::health(25)),
            ("h050", State::health(50)),
            ("h075", State::health(75)),
            ("h100", full(100, 60, false)),
            ("h125", full(125, 100, true)),
            ("a200", full(100, 200, false)),
            ("a300", full(100, 300, true)),
            ("v1", super::tests::v1_state(72)),
            (
                "factorio",
                parse(
                    "150/250 shield 30/150 battery 25 research 37 technology military attack 0",
                    SystemTime::now(),
                    SystemTime::now(),
                )
                .unwrap(),
            ),
            (
                "terraria",
                parse(
                    "240/400 mana 80/200 defense 45 breath 80/200",
                    SystemTime::now(),
                    SystemTime::now(),
                )
                .unwrap(),
            ),
        ] {
            let mut m = Meter::default();
            if tag == "v1" {
                m.tuning = Tuning::parse("sprite v1\nheart 2 1\n");
            }
            for i in 0..40 {
                pbm(&dir, &format!("{tag}-{i:03}.pbm"), &m.frame(state));
            }
        }
        // The demo: the states in order, a few seconds each, one meter throughout.
        let rest = [0, 145, 255];
        let script = [
            (State::Wait, 25),
            (full(100, 100, true), 25),
            (full(88, 40, true), 15),
            (full(72, 0, false), 20),
            (full(55, 0, false), 15),
            (full(44, 0, false), 20),
            (full(22, 0, false), 30),
            (full(9, 0, false), 20),
            (State::health(0), 55),
            (full(130, 100, false), 30),
        ];
        let mut m = Meter::default();
        let mut colours = String::new();
        let mut i = 0;
        for (state, ticks) in script {
            for _ in 0..ticks {
                pbm(&dir, &format!("demo-{i:03}.pbm"), &m.frame(state));
                let [r, g, b] = m.colour(state, rest);
                colours.push_str(&format!("{r} {g} {b}\n"));
                i += 1;
            }
        }
        fs::write(dir.join("demo.colours"), colours).unwrap();
    }
}

// ---- a game's console log ----

/// How long a log line lives in the feed; the ACS script repeats itself every ~5 s.
const LOG_TTL: Duration = Duration::from_secs(15);

/// The state in a `G13HEALTH HEALTH MAX ARMOR` console line, if the line is one.
pub fn log_line(line: &str) -> Option<State> {
    let rest = line.split("G13HEALTH ").nth(1)?;
    let mut w = rest.split_whitespace();
    let health: f64 = w.next()?.parse().ok()?;
    let max: f64 = w.next()?.parse().ok()?;
    let armor: f64 = w.next()?.parse().ok()?;
    if max <= 0.0 || health < 0.0 || armor < 0.0 {
        return None;
    }
    Some(State::Health {
        pct: (health / max * 100.0).round().clamp(0.0, 999.0) as u32,
        // Doom's green armour is 100, blue 200: the bar thickens past 100.
        shield: armor.round().clamp(0.0, 999.0) as u32,
        helmet: false,
        extra: Extra::NONE,
    })
}

/// The newest file whose name starts with `path`'s (Zandronum appends a timestamp).
fn newest_log(path: &std::path::Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_name()?.to_str()?;
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(stem)))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()
        .map(|(_, p)| p)
}

/// Follows the log until `stop`: the last tagged line already there gives the first
/// state, new lines give the rest; a new or truncated file is picked up within a second.
fn follow_log(path: &std::path::Path, stop: &AtomicBool) {
    use std::io::{BufRead, Seek, SeekFrom};
    let mut open: Option<(PathBuf, std::io::BufReader<fs::File>, u64)> = None;
    let mut last: Option<State> = None;
    let mut announced = false;
    // The script prints only on change; the feed's ttl is kept alive from here.
    let mut kept = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let newest = newest_log(path);
        if open.as_ref().map(|(p, _, _)| p) != newest.as_ref() {
            open = newest.and_then(|p| {
                let f = fs::File::open(&p).ok()?;
                eprintln!("health meter: following {}", p.display());
                announced = true;
                Some((p, std::io::BufReader::new(f), 0))
            });
            last = None;
        }
        if !announced {
            eprintln!("health meter: no log yet at {}", path.display());
            announced = true;
        }
        let mut changed = None;
        if let Some((p, reader, seen)) = &mut open {
            // Truncated (a new game over the same name): start again from the top.
            if fs::metadata(&*p).map(|m| m.len()).unwrap_or(0) < *seen {
                let _ = reader.seek(SeekFrom::Start(0));
                *seen = 0;
            }
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                *seen += n as u64;
                if let Some(s) = log_line(&line) {
                    changed = Some(s);
                }
                line.clear();
            }
        }
        if changed.is_some() {
            last = changed;
        }
        if let Some(s) = last {
            if changed.is_some() || kept.elapsed() >= LOG_TTL / 3 {
                if let Err(e) = write(Some(s), Some(LOG_TTL)) {
                    eprintln!("health meter: {e}");
                }
                kept = Instant::now();
            }
        }
        thread::sleep(TICK);
    }
}

// ---- Source engine games ----

/// The LCD page a Source 2013 client renders when started with `-g15`: the title page
/// says `G13 wait`, the player page spells the health line that `g15.so`
/// (`contrib/source-health`) turns into the feed.
pub const SOURCE_RES: &str = include_str!("../contrib/source-health/g15.res");

/// A Source engine game folder as Steam lays it out: `bin/` with the engine (under
/// `linux64/` for a 64-bit client), and one or more mod folders with a `gameinfo.txt`
/// (`cstrike`, `hl2`, `episodic`, ...), each of which mounts `custom/*`.
#[derive(Debug)]
pub struct SourceGame {
    pub bits: u8,
    /// Where the engine looks for the module: `bin/g15.so` (32-bit) or
    /// `bin/linux64/bin/g15.so` (64-bit, its platform folder plus the name it asks for).
    pub module: PathBuf,
    pub mods: Vec<PathBuf>,
    /// From the mods' `gameinfo.txt` `type` keys: all `singleplayer_only` makes the game
    /// singleplayer; anything else, or nothing said, multiplayer, the safe side (the sp
    /// module patches the client's vtable for armour, which no anti-cheat should see).
    pub kind: SourceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Single,
    Multi,
}

impl SourceKind {
    pub fn word(self) -> &'static str {
        match self {
            SourceKind::Single => "sp",
            SourceKind::Multi => "mp",
        }
    }
    pub fn parse(word: &str) -> Option<SourceKind> {
        match word {
            "sp" | "single" | "singleplayer" => Some(SourceKind::Single),
            "mp" | "multi" | "multiplayer" => Some(SourceKind::Multi),
            _ => None,
        }
    }
}

/// The `type` key of a gameinfo.txt (`singleplayer_only`, `multiplayer_only`), if any.
fn gameinfo_type(mod_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(mod_dir.join("gameinfo.txt")).ok()?;
    text.lines().find_map(|l| {
        let mut w = l.split_whitespace();
        (w.next()? == "type").then(|| w.next().unwrap_or("").trim_matches('"').to_string())
    })
}

pub fn source_game(root: &std::path::Path) -> Result<SourceGame, String> {
    use std::io::Read;
    let (engine, module) = if root.join("bin/linux64/engine.so").is_file() {
        (
            root.join("bin/linux64/engine.so"),
            root.join("bin/linux64/bin/g15.so"),
        )
    } else if root.join("bin/engine.so").is_file() {
        (root.join("bin/engine.so"), root.join("bin/g15.so"))
    } else {
        return Err(format!(
            "{}: no bin/engine.so or bin/linux64/engine.so here; give the game's own folder, \
             the one holding bin/ and the mod folder (cstrike, hl2, ...)",
            root.display()
        ));
    };
    let mut head = [0u8; 5];
    fs::File::open(&engine)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| format!("{}: {e}", engine.display()))?;
    let bits = match head {
        [0x7f, b'E', b'L', b'F', 1] => 32,
        [0x7f, b'E', b'L', b'F', 2] => 64,
        _ => return Err(format!("{}: not an ELF library", engine.display())),
    };
    let mut mods: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("gameinfo.txt").is_file())
        .collect();
    mods.sort();
    if mods.is_empty() {
        return Err(format!(
            "{}: no mod folder with a gameinfo.txt",
            root.display()
        ));
    }
    let types: Vec<String> = mods.iter().filter_map(|m| gameinfo_type(m)).collect();
    let kind = if types.len() == mods.len() && types.iter().all(|t| t == "singleplayer_only") {
        SourceKind::Single
    } else {
        SourceKind::Multi
    };
    Ok(SourceGame {
        bits,
        module,
        mods,
        kind,
    })
}

/// The built module for a client of `bits`: `G13MAP_G15_DIR`, then
/// `lib/g13pad/source-health` under the executable's prefix, then the usual prefixes.
fn g15_module(kind: SourceKind, bits: u8) -> Result<PathBuf, String> {
    let name = format!(
        "g15-{}-{}.so",
        kind.word(),
        if bits == 64 { "x86_64" } else { "i386" }
    );
    let mut dirs = Vec::new();
    if let Some(d) = env::var_os("G13MAP_G15_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(prefix) = env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .and_then(|bin| bin.parent().map(Path::to_path_buf))
    }) {
        dirs.push(prefix.join("lib/g13pad/source-health"));
    }
    dirs.push(PathBuf::from("/usr/lib/g13pad/source-health"));
    dirs.push(PathBuf::from("/usr/local/lib/g13pad/source-health"));
    dirs.iter()
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "{name} not found in {}; build the source-health component or set G13MAP_G15_DIR",
                dirs.iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn res_path(mod_dir: &Path) -> PathBuf {
    mod_dir.join("custom/g13pad/resource/g15.res")
}

/// `g13map health source DIR [sp|mp]`: the module (of the game's kind unless told
/// otherwise) into the game's bin folder, the page into every mod folder's `custom/`,
/// and what is left to do by hand.
pub fn source_install(root: &Path, kind: Option<SourceKind>) -> Result<String, String> {
    let game = source_game(root)?;
    let kind = kind.unwrap_or(game.kind);
    let module = g15_module(kind, game.bits)?;
    if let Some(d) = game.module.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // Beside, then renamed over: a running game keeps the inode it mapped.
    let staged = game.module.with_extension("so.new");
    fs::copy(&module, &staged)
        .and_then(|_| fs::rename(&staged, &game.module))
        .map_err(|e| format!("{} -> {}: {e}", module.display(), game.module.display()))?;
    let mut out = format!(
        "{}-bit {} client: {} ({})\n",
        game.bits,
        match kind {
            SourceKind::Single => "singleplayer",
            SourceKind::Multi => "multiplayer",
        },
        game.module.display(),
        match kind {
            SourceKind::Single => "health and armour; hooks the client's Battery message",
            SourceKind::Multi => "health only; touches nothing of the game's",
        }
    );
    for m in &game.mods {
        let res = res_path(m);
        let dir = res.parent().unwrap();
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let staged = res.with_extension("res.new");
        fs::write(&staged, SOURCE_RES)
            .and_then(|_| fs::rename(&staged, &res))
            .map_err(|e| format!("{}: {e}", res.display()))?;
        out.push_str(&format!("page: {}\n", res.display()));
    }
    out.push_str(
        "Now add -g15 to the game's launch options in Steam, and put its profile in health \
         mode feed (g13map profile health NAME feed). Remove with: g13map health source DIR remove",
    );
    Ok(out)
}

/// `g13map health source DIR remove`: only what `source_install` put there.
pub fn source_remove(root: &Path) -> Result<String, String> {
    let game = source_game(root)?;
    let mut removed = Vec::new();
    let mut gone = |p: &Path| -> Result<(), String> {
        match fs::remove_file(p) {
            Ok(()) => {
                removed.push(p.display().to_string());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    };
    gone(&game.module)?;
    for m in &game.mods {
        let res = res_path(m);
        gone(&res)?;
        // Our folders only, and only while empty.
        let _ = fs::remove_dir(res.parent().unwrap());
        let _ = fs::remove_dir(m.join("custom/g13pad"));
    }
    if game.bits == 64 {
        let _ = fs::remove_dir(game.module.parent().unwrap());
    }
    Ok(if removed.is_empty() {
        "nothing of ours there".into()
    } else {
        format!("removed:\n{}", removed.join("\n"))
    })
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::test_support::Sandbox;

    /// A game folder with an engine of `bits` and the named mod folders.
    fn game(dir: &Path, name: &str, bits: u8, mods: &[&str]) -> PathBuf {
        let root = dir.join(name);
        let engine = if bits == 64 {
            root.join("bin/linux64/engine.so")
        } else {
            root.join("bin/engine.so")
        };
        fs::create_dir_all(engine.parent().unwrap()).unwrap();
        fs::write(
            &engine,
            [0x7f, b'E', b'L', b'F', if bits == 64 { 2 } else { 1 }, 0],
        )
        .unwrap();
        for m in mods {
            fs::create_dir_all(root.join(m)).unwrap();
            fs::write(root.join(m).join("gameinfo.txt"), "\"GameInfo\" {}\n").unwrap();
        }
        root
    }

    #[test]
    fn the_module_follows_the_engines_class_and_the_page_every_mod() {
        let sandbox = Sandbox::new("source");
        let modules = sandbox.dir.join("modules");
        fs::create_dir_all(&modules).unwrap();
        for (name, body) in [
            ("g15-mp-x86_64.so", "sixty-four mp"),
            ("g15-sp-x86_64.so", "sixty-four sp"),
            ("g15-mp-i386.so", "thirty-two mp"),
            ("g15-sp-i386.so", "thirty-two sp"),
        ] {
            fs::write(modules.join(name), body).unwrap();
        }
        let saved = env::var_os("G13MAP_G15_DIR");
        env::set_var("G13MAP_G15_DIR", &modules);

        let css = game(&sandbox.dir, "Counter-Strike Source", 64, &["cstrike"]);
        fs::write(
            css.join("cstrike/gameinfo.txt"),
            "\"GameInfo\"\n{\n\ttype multiplayer_only\n}\n",
        )
        .unwrap();
        let report = source_install(&css, None).unwrap();
        assert!(report.starts_with("64-bit multiplayer client"), "{report}");
        assert_eq!(
            fs::read(css.join("bin/linux64/bin/g15.so")).unwrap(),
            b"sixty-four mp"
        );
        assert_eq!(
            fs::read_to_string(css.join("cstrike/custom/g13pad/resource/g15.res")).unwrap(),
            SOURCE_RES
        );
        assert!(SOURCE_RES.contains("G13 wait") && SOURCE_RES.contains("%(localplayer)m_iHealth%"));

        let hl2 = game(&sandbox.dir, "Half-Life 2", 32, &["hl2", "episodic", "ep2"]);
        fs::write(hl2.join("hl2/custom/readme.txt"), "theirs").unwrap_or_else(|_| {
            fs::create_dir_all(hl2.join("hl2/custom")).unwrap();
            fs::write(hl2.join("hl2/custom/readme.txt"), "theirs").unwrap();
        });
        for m in ["hl2", "episodic", "ep2"] {
            fs::write(
                hl2.join(m).join("gameinfo.txt"),
                "\"GameInfo\"\n{\n\ttype\t\t\"singleplayer_only\"\n}\n",
            )
            .unwrap();
        }
        let report = source_install(&hl2, None).unwrap();
        assert!(report.starts_with("32-bit singleplayer client"), "{report}");
        assert_eq!(fs::read(hl2.join("bin/g15.so")).unwrap(), b"thirty-two sp");
        // Told otherwise, the safe module goes in; a mixed or unlabelled root is multiplayer.
        source_install(&hl2, Some(SourceKind::Multi)).unwrap();
        assert_eq!(fs::read(hl2.join("bin/g15.so")).unwrap(), b"thirty-two mp");
        fs::write(hl2.join("ep2/gameinfo.txt"), "\"GameInfo\" {}\n").unwrap();
        assert_eq!(source_game(&hl2).unwrap().kind, SourceKind::Multi);
        source_install(&hl2, None).unwrap();
        for m in ["hl2", "episodic", "ep2"] {
            assert!(
                hl2.join(m).join("custom/g13pad/resource/g15.res").is_file(),
                "{m}"
            );
        }

        // Removal takes only ours and leaves the game's own custom files.
        assert!(source_remove(&hl2).unwrap().starts_with("removed:"));
        assert!(!hl2.join("bin/g15.so").exists());
        assert!(!hl2.join("hl2/custom/g13pad").exists());
        assert!(hl2.join("hl2/custom/readme.txt").is_file());
        assert!(hl2.join("bin/engine.so").is_file());
        assert_eq!(source_remove(&hl2).unwrap(), "nothing of ours there");
        source_remove(&css).unwrap();
        assert!(!css.join("bin/linux64/bin").exists());
        assert!(css.join("bin/linux64/engine.so").is_file());

        match saved {
            Some(v) => env::set_var("G13MAP_G15_DIR", v),
            None => env::remove_var("G13MAP_G15_DIR"),
        }
    }

    #[test]
    fn folders_that_are_not_a_game_are_refused() {
        let sandbox = Sandbox::new("source-refuse");
        let err = source_install(&sandbox.dir, None).unwrap_err();
        assert!(err.contains("no bin/engine.so"), "{err}");
        let no_mod = game(&sandbox.dir, "bare", 64, &[]);
        assert!(source_game(&no_mod).unwrap_err().contains("gameinfo.txt"));
        let odd = game(&sandbox.dir, "odd", 64, &["mod"]);
        fs::write(odd.join("bin/linux64/engine.so"), b"not an elf").unwrap();
        assert!(source_game(&odd).unwrap_err().contains("not an ELF"));
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[test]
    fn console_lines_become_states() {
        assert_eq!(
            log_line("G13HEALTH 87 100 50"),
            Some(State::Health {
                pct: 87,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        assert_eq!(
            log_line("[00:51:59] G13HEALTH 200 100 200\n"),
            Some(State::Health {
                pct: 200,
                shield: 200,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        assert_eq!(log_line("G13HEALTH 0 100 0"), Some(State::health(0)));
        for bad in [
            "G13HEALTH 1 0 0",
            "G13HEALTH x 100 0",
            "health 1 2 3",
            "G13HEALTH 1 100",
        ] {
            assert_eq!(log_line(bad), None, "{bad}");
        }
    }
    #[test]
    fn the_newest_matching_log_is_followed_into_the_feed() {
        let _box = crate::test_support::Sandbox::new("meter-log");
        prepare();
        let base = crate::state_dir().join("game.log");
        assert_eq!(newest_log(&base), None);
        fs::write(
            crate::state_dir().join("game.log__old"),
            "G13HEALTH 50 100 0\n",
        )
        .unwrap();
        thread::sleep(Duration::from_millis(20));
        let new = crate::state_dir().join("game.log__new");
        fs::write(&new, "noise\nG13HEALTH 80 100 20\n").unwrap();
        assert_eq!(newest_log(&base), Some(new.clone()));
        let l = Listener::follow(base.clone());
        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            read(),
            Some(State::Health {
                pct: 80,
                shield: 20,
                helmet: false,
                extra: Extra::NONE,
            })
        );
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&new).unwrap();
        writeln!(f, "G13HEALTH 0 100 0").unwrap();
        drop(f);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(read(), Some(State::health(0)));
        drop(l);
    }
}
