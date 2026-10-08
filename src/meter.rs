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
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Connected, no health yet.
    Wait,
    /// Health and shield, both in percent (health may exceed 100), and whether the head
    /// is covered too (CS2's helmet; asked 2026-10-08): the shield bar is solid then.
    Health { pct: u32, shield: u32, helmet: bool },
}

impl State {
    pub fn health(pct: u32) -> State {
        State::Health {
            pct,
            shield: 0,
            helmet: false,
        }
    }
    pub fn band(self) -> Option<Band> {
        match self {
            State::Wait => None,
            State::Health { pct, .. } => Some(Band::of(pct)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Dead,
    Red,
    Orange,
    Yellow,
    Green,
    Shield,
}

impl Band {
    pub fn of(pct: u32) -> Band {
        match pct {
            0 => Band::Dead,
            1..=25 => Band::Red,
            26..=50 => Band::Orange,
            51..=75 => Band::Yellow,
            76..=100 => Band::Green,
            _ => Band::Shield,
        }
    }
    /// The band's default backlight.
    pub fn rgb(self) -> [u8; 3] {
        Tuning::default().rgb(self)
    }
    const ALL: [Band; 6] = [
        Band::Dead,
        Band::Red,
        Band::Orange,
        Band::Yellow,
        Band::Green,
        Band::Shield,
    ];
    fn key(self) -> &'static str {
        match self {
            Band::Dead => "dead",
            Band::Red => "red",
            Band::Orange => "orange",
            Band::Yellow => "yellow",
            Band::Green => "green",
            Band::Shield => "blue",
        }
    }
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
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    /// Backlights in `Band::ALL` order. The G13's LED makes yellow and orange from red
    /// plus a little green; full green in the mix reads lime.
    pub colours: [[u8; 3]; 6],
    /// Seconds the dark flatline holds after a drop to zero.
    pub hold: f32,
    /// Seconds from beat to beat at full health, and on the last point.
    pub calm: f32,
    pub racing: f32,
    /// Whether the red band flashes the panel on each beat.
    pub flash: bool,
    /// The loopback port the Counter-Strike 2 listener takes.
    pub cs2_port: u16,
    /// The console log a `health log` profile follows; the newest file whose name
    /// starts with it (Zandronum adds a timestamp to the name).
    pub log_file: PathBuf,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            colours: [
                [0, 0, 0],
                [255, 0, 0],
                [255, 48, 0],
                [255, 215, 0],
                [0, 255, 0],
                [0, 64, 255],
            ],
            hold: 3.0,
            calm: 5.0,
            racing: 1.4,
            flash: true,
            cs2_port: CS2_PORT,
            log_file: crate::state_dir().join("game.log"),
        }
    }
}

impl Tuning {
    pub fn path() -> PathBuf {
        crate::config_dir().join("meter")
    }
    pub fn load() -> Tuning {
        fs::read_to_string(Self::path())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
    pub fn parse(text: &str) -> Tuning {
        let mut t = Tuning::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut words = line.split_whitespace();
            let Some(key) = words.next() else { continue };
            let values: Vec<&str> = words.collect();
            let secs = |v: &[&str]| v.first().and_then(|s| s.parse::<f32>().ok());
            match key {
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
                "flash" => {
                    t.flash = match values.first() {
                        Some(&"on") => true,
                        Some(&"off") => false,
                        _ => t.flash,
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
                    if let Some(i) = Band::ALL.iter().position(|b| b.key() == key) {
                        let rgb: Vec<u8> = values.iter().filter_map(|v| v.parse().ok()).collect();
                        if let [r, g, b] = rgb[..] {
                            t.colours[i] = [r, g, b];
                        }
                    }
                }
            }
        }
        t
    }
    /// The file, commented, as `prepare` writes it the first time.
    pub fn to_text(&self) -> String {
        let mut s = String::from(
            "# The G13 health meter's look. g13map watch re-reads this within two seconds.\n\
             # Backlight per band, R G B 0-255. Bands: dead 0, red 1-25, orange 26-50,\n\
             # yellow 51-75, green 76-100, blue over 100.\n",
        );
        for (band, [r, g, b]) in Band::ALL.iter().zip(self.colours) {
            s.push_str(&format!("{} {r} {g} {b}\n", band.key()));
        }
        s.push_str(&format!(
            "# Seconds the dark flatline holds after a drop to zero.\nhold {}\n\
             # Seconds from beat to beat at full health, and on the last point.\ncalm {}\nracing {}\n\
             # on: the red band flashes the panel on each beat.\nflash {}\n\
             # The loopback port for a profile with `health cs2` (the game's cfg must match).\ncs2_port {}\n\
             # The console log a profile with `health log` follows (newest file starting with it).\nlog_file {}\n",
            self.hold,
            self.calm,
            self.racing,
            if self.flash { "on" } else { "off" },
            self.cs2_port,
            self.log_file.display()
        ));
        s
    }
    pub fn rgb(&self, band: Band) -> [u8; 3] {
        self.colours[Band::ALL.iter().position(|b| *b == band).unwrap_or(0)]
    }
    fn hold_ticks(&self) -> u32 {
        (self.hold / TICK.as_secs_f32()).round().max(1.0) as u32
    }
    /// Columns from one beat to the next at `pct`: the trace runs `SCROLL` columns a
    /// tick, so seconds times columns a second, between racing and calm.
    fn period(&self, pct: u32) -> i32 {
        let cols = |secs: f32| (secs * SCROLL as f32 / TICK.as_secs_f32()).round() as i32;
        let (racing, calm) = (cols(self.racing), cols(self.calm));
        (racing + (calm - racing) * pct.min(100) as i32 / 100).max(60)
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
    let p = Tuning::path();
    if !p.exists() {
        let _ = fs::create_dir_all(crate::config_dir());
        let _ = fs::write(p, Tuning::default().to_text());
    }
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
                State::Wait => return None,
            },
            "helmet" => match (&mut state, value) {
                (State::Health { helmet, .. }, "on") => *helmet = true,
                (State::Health { helmet, .. }, "off") => *helmet = false,
                _ => return None,
            },
            "ttl" => {
                let ttl = value.parse::<f64>().ok().filter(|s| *s > 0.0)?;
                if now.duration_since(written).unwrap_or_default() > Duration::from_secs_f64(ttl) {
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
        State::Health {
            pct,
            shield,
            helmet,
        } => format!(
            "{pct} shield {shield}{}",
            if helmet { " helmet on" } else { "" }
        ),
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
/// The column of the spike's top in `ecg`.
const R_PEAK: i32 = 33;
const HEART_X: i32 = 4;
const HEART_Y: i32 = 3;

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
    for (i, c) in text.chars().enumerate() {
        let g = match c {
            '0'..='9' => &GLYPHS[c as usize - '0' as usize],
            _ => &GLYPHS[10],
        };
        for (r, row) in g.iter().enumerate() {
            for (k, cell) in row.bytes().enumerate() {
                if cell == b'#' {
                    let (x, y) = (x0 + i as i32 * 12 + k as i32 * 2, y + r as i32 * 2);
                    bm.plot(x, y);
                    bm.plot(x + 1, y);
                    bm.plot(x, y + 1);
                    bm.plot(x + 1, y + 1);
                }
            }
        }
    }
}

const BAR_X: i32 = 4;
const BAR_W: i32 = 152;

/// The health bar along the bottom with notches at the band edges, and the shield bar
/// above it when there is any shield: hatched, or solid when the head is covered too.
fn bars(bm: &mut Bitmap, fill: i32, shield: i32, solid: bool) {
    for x in 2..=157 {
        bm.plot(x, 34);
        bm.plot(x, 42);
    }
    for y in 34..=42 {
        bm.plot(2, y);
        bm.plot(157, y);
    }
    for k in 1..4 {
        let x = BAR_X + BAR_W * k / 4;
        bm.put(x, 34, false);
        bm.put(x, 42, false);
    }
    for x in BAR_X..BAR_X + fill.clamp(0, BAR_W) {
        for y in 36..=40 {
            bm.plot(x, y);
        }
    }
    for x in BAR_X..BAR_X + shield.clamp(0, BAR_W) {
        for y in 30..=32 {
            if solid || (x + y) % 2 == 0 {
                bm.plot(x, y);
            }
        }
    }
}

/// The meter's picture as it runs: the trace as a ring of column lifts, scrolled left
/// `SCROLL` columns a tick with new columns from the beat clock on the right. A pure
/// function of the ticks and states it was given, so tests can draw it without a clock.
pub struct Meter {
    lift: [i32; W],
    /// Columns since the current beat began.
    u: i32,
    /// Ticks since the last beat began; `None` before the first.
    beat_age: Option<u32>,
    /// Ticks at zero health.
    dead: u32,
    tick: u32,
    /// The look; the frame thread takes a new one from the file.
    pub tuning: Tuning,
}

impl Default for Meter {
    fn default() -> Self {
        Meter {
            lift: [0; W],
            u: 0,
            beat_age: None,
            dead: 0,
            tick: 0,
            tuning: Tuning::default(),
        }
    }
}

impl Meter {
    /// Advances one tick in `state` and draws it.
    pub fn frame(&mut self, state: State) -> Bitmap {
        let mut bm = Bitmap::blank();
        self.tick = self.tick.wrapping_add(1);
        // A drop to zero always holds the dark flatline for the whole hold, whatever the
        // feed says meanwhile (asked 2026-10-07); only then does the next state show.
        let zero = matches!(state, State::Health { pct: 0, .. });
        self.dead = if zero || self.holding() {
            self.dead + 1
        } else {
            0
        };
        match state {
            _ if self.holding() => self.alive(&mut bm, 0, 0, false),
            State::Wait => self.waiting(&mut bm),
            State::Health { pct: 0, .. } => self.waiting(&mut bm),
            State::Health {
                pct,
                shield,
                helmet,
            } => self.alive(&mut bm, pct, shield, helmet),
        }
        bm
    }

    /// Inside the dark seconds after a drop to zero.
    fn holding(&self) -> bool {
        (1..=self.tuning.hold_ticks()).contains(&self.dead)
    }

    /// The backlight for the frame just drawn: off through the hold, the band's colour,
    /// the resting colour while waiting, red once a flatline has turned into the search
    /// for a pulse.
    pub fn colour(&self, state: State, rest: [u8; 3]) -> [u8; 3] {
        if self.holding() {
            return self.tuning.rgb(Band::Dead);
        }
        match state.band() {
            None => rest,
            Some(Band::Dead) => self.tuning.rgb(Band::Red),
            Some(band) => self.tuning.rgb(band),
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
            beat |= beating && self.u == R_PEAK;
            self.u += 1;
            if beating && self.u >= period {
                self.u = 0;
            }
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

    fn alive(&mut self, bm: &mut Bitmap, pct: u32, shield: u32, helmet: bool) {
        self.scroll(pct);
        self.trace(bm);
        let band = Band::of(pct);
        if band == Band::Dead {
            bm.sprite(HEART_X, HEART_Y, HEART_OUTLINE);
        } else if matches!(self.beat_age, Some(0..=2)) {
            bm.sprite(HEART_X - 1, HEART_Y - 1, HEART_BIG);
        } else {
            bm.sprite(HEART_X, HEART_Y, HEART);
        }
        if shield > 0 {
            // A ring around the heart, the shield's own mark.
            bm.line(1, 0, 13, 0);
            bm.line(1, 11, 13, 11);
            bm.line(0, 1, 0, 10);
            bm.line(14, 1, 14, 10);
        }
        readout(bm, &pct.min(999).to_string(), 157, 2);
        bars(
            bm,
            (BAR_W * pct.min(100) as i32 + 50) / 100,
            (BAR_W * shield.min(100) as i32 + 50) / 100,
            helmet,
        );
        // On the last quarter every beat flashes the whole panel: the alarm.
        if self.tuning.flash && band == Band::Red && self.beat_age == Some(0) {
            for y in 0..H {
                for x in 0..W {
                    bm.set(x, y, !bm.get(x, y));
                }
            }
        }
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
        if t % 20 < 3 {
            bm.sprite(HEART_X, HEART_Y, HEART);
        } else {
            bm.sprite(HEART_X, HEART_Y, HEART_OUTLINE);
        }
        readout(bm, "--", 157, 2);
        bars(bm, 0, 0, false);
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
    pub fn start(state: State, rest: [u8; 3], reader: Reader) -> Live {
        let tuning = Tuning::load();
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
    let mut meter = Meter::default();
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
    let lcd = crate::load(&active).ok().and_then(|(p, _)| p.lcd);
    if selects(lcd.as_deref()).is_none() {
        println!(
            "note: profile '{active}' does not show the health meter (g13map profile lcd \
             '{active}' health); the panel will not follow this run"
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
            }),
            3000,
        ),
        (
            "90",
            Some(State::Health {
                pct: 90,
                shield: 20,
                helmet: false,
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
            }),
            4000,
        ),
        (
            "150",
            Some(State::Health {
                pct: 150,
                shield: 100,
                helmet: false,
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
        let edges = [
            (0, Band::Dead),
            (1, Band::Red),
            (25, Band::Red),
            (26, Band::Orange),
            (50, Band::Orange),
            (51, Band::Yellow),
            (75, Band::Yellow),
            (76, Band::Green),
            (100, Band::Green),
            (101, Band::Shield),
            (999, Band::Shield),
        ];
        for (pct, band) in edges {
            assert_eq!(Band::of(pct), band, "{pct}");
        }
    }

    #[test]
    fn tuning_file_round_trips_and_tolerates_junk() {
        let d = Tuning::default();
        assert_eq!(Tuning::parse(&d.to_text()), d);
        assert_eq!(Tuning::parse(""), d);
        let t = Tuning::parse(
            "yellow 255 255 0 # more yellow\nhold 1.5\nflash off\ncalm abc\nracing 0\nnonsense 1 2 3\nred 1 2\n",
        );
        assert_eq!(t.rgb(Band::Yellow), [255, 255, 0]);
        assert_eq!(t.rgb(Band::Red), d.rgb(Band::Red));
        assert_eq!(t.hold, 1.5);
        assert!(!t.flash);
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
        assert_eq!(d.period(100), 250);
        assert_eq!(d.period(0), 70);
        assert_eq!(
            Tuning {
                calm: 0.5,
                ..d.clone()
            }
            .period(100),
            60
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
                helmet: false
            })
        );
        assert_eq!(
            p("80 shield 50 helmet on"),
            Some(State::Health {
                pct: 80,
                shield: 50,
                helmet: true
            })
        );
        assert_eq!(
            p("80 shield 50 helmet off"),
            Some(State::Health {
                pct: 80,
                shield: 50,
                helmet: false
            })
        );
        assert_eq!(p("80 helmet yes"), None);
        assert_eq!(p("wait helmet on"), None);
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
        write(
            Some(State::Health {
                pct: 42,
                shield: 7,
                helmet: false,
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            read(),
            Some(State::Health {
                pct: 42,
                shield: 7,
                helmet: false
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
                helmet: false
            })
        );
        thread::sleep(Duration::from_millis(20));
        write(Some(State::health(70)), None).unwrap();
        assert_eq!(read(), Some(State::health(70)));
        write(None, None).unwrap();
        assert!(!state.exists());
        assert_eq!(read(), None);
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
                helmet: false
            }) > 200
        );
        // A helmet makes the shield bar solid: every pixel of its three rows.
        assert_eq!(
            shield(State::Health {
                pct: 100,
                shield: 100,
                helmet: true
            }),
            152 * 3
        );
        // A flatline never beats and holds still, dark, for three seconds; then the
        // search for a pulse begins under red, and any health ends the count.
        let mut m = Meter::default();
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
        assert_eq!(colours[31], Band::Red.rgb());
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
            let want = next.band().map_or([9, 9, 9], Band::rgb);
            assert_eq!(m.colour(next, [9, 9, 9]), want, "{next:?} colour after");
        }
        // Full health beats every 250 columns, 50 ticks.
        let mut m = Meter::default();
        let beats: Vec<u32> = (0..200u32)
            .filter(|_| {
                m.frame(State::health(100));
                m.beat_age == Some(0)
            })
            .collect();
        assert_eq!(beats, vec![6, 56, 106, 156]);
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
                helmet: false
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
                helmet: true
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
/// `demo-NNN.pbm` with `demo.colours` (one `R G B` backlight per frame), for a GIF.
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
        ] {
            let mut m = Meter::default();
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
        // Doom's green armour is 100, blue 200: full bar from green up.
        shield: armor.round().clamp(0.0, 100.0) as u32,
        helmet: false,
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
                helmet: false
            })
        );
        assert_eq!(
            log_line("[00:51:59] G13HEALTH 200 100 200\n"),
            Some(State::Health {
                pct: 200,
                shield: 100,
                helmet: false
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
                helmet: false
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
