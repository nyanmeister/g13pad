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

mod state;
pub use state::{Extra, Label, Resource, State, RANKS};
mod tuning;
pub use tuning::{Band, Flash, Sprite, Tuning, VehicleStyle};
mod feed;
pub use feed::{parse, path, paths, prepare, read, write};
mod render;

pub(crate) mod parts;
pub(crate) use render::text;
pub use render::{Meter, POLL, TICK};
mod runtime;
pub use runtime::Live;
mod demo;
pub use demo::demo;
mod transport;
pub use transport::{cs2, cs2_config, cs2_state, log_line, Listener, CS2_PORT};
mod source;
pub use source::{source_game, source_install, source_remove, SourceGame, SourceKind, SOURCE_RES};

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
    /// RetroArch's local achievement log, independent of the health feed.
    RetroArch,
    /// Dolphin's local achievement log, independent of the health feed.
    Dolphin,
}

impl Reader {
    /// The word in a profile's `# health` line and in `g13map profile health`.
    pub fn key(self) -> &'static str {
        match self {
            Reader::Feed => "feed",
            Reader::Cs2 => "cs2",
            Reader::Log => "log",
            Reader::RetroArch => "retroarch",
            Reader::Dolphin => "dolphin",
        }
    }
    pub fn parse(word: &str) -> Option<Reader> {
        [
            Reader::Feed,
            Reader::Cs2,
            Reader::Log,
            Reader::RetroArch,
            Reader::Dolphin,
        ]
        .into_iter()
        .find(|r| r.key() == word)
    }
    /// What the reader is, for a label.
    pub fn about(self) -> &'static str {
        match self {
            Reader::Feed => "a mod or script feeds it",
            Reader::Cs2 => "Counter-Strike 2 listener",
            Reader::Log => "a game's console log (Doom)",
            Reader::RetroArch => "RetroArch achievements (local log)",
            Reader::Dolphin => "Dolphin achievements (local log)",
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
pub const NAMES: [(&str, Reader); 5] = [
    ("health", Reader::Feed),
    ("health cs2", Reader::Cs2),
    ("health log", Reader::Log),
    ("ra retroarch", Reader::RetroArch),
    ("ra dolphin", Reader::Dolphin),
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
    match selects(Some(name))? {
        Reader::RetroArch => return Some(crate::ra::preview(crate::ra::Source::RetroArch)),
        Reader::Dolphin => return Some(crate::ra::preview(crate::ra::Source::Dolphin)),
        _ => {}
    }
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

#[cfg(test)]
mod tests;
