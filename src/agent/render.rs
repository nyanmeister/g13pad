// SPDX-License-Identifier: GPL-3.0-or-later
//! Drawing the agent screen (layout B, chosen 2026-10-15): a big pulse and the activity
//! word as the headline, the turn stopwatch in hundredths, the background-agent count and
//! the prior turn's length below, with the backlight carrying the alert states. Composed
//! from the shared glyph font (`meter::text`) and the Draw kit; the heart is its own.
//!
//! Two clocks drive it: `age` is how long the current phase has been showing (so a blink
//! restarts on entry and the idle breath is anchored), `now` is the wall clock (so the
//! stopwatch ticks and the fail/deny flashes land on their real timestamps). The pulse
//! runs off the wall clock too, so it does not jump when the phase changes.
use super::State;
use crate::draw::Draw;
use crate::lcd::{Bitmap, W};
use crate::meter::text;
use std::time::{Duration, SystemTime};

/// The relaxed heart (9x8) and its contraction on the beat (7x6, centred in the same box).
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
const HEART: &[&str] = &[
    ".##.##.", "#######", "#######", ".#####.", "..###..", "...#...",
];

// Working backlight is the profile's own colour; the alerts override it.
// Saturated for the backlight: the G13's RGB LED is additive, so any light in all three
// channels at once washes to white. Zero the channels a hue does not use (asked 2026-10-15:
// the first, pastel set read as white on the lamp). These are a by-eye starting point.
const YELLOW: [u8; 3] = [255, 200, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const ORANGE: [u8; 3] = [255, 85, 0];
const RED: [u8; 3] = [255, 0, 0];

/// One beat of the pulse: a short contraction, then relaxed.
const BEAT: u128 = 520;
const CONTRACT: u128 = 120;
/// Half a blink: the inversion is on for this long, off for this long (permission).
const BLINK_HALF: u128 = 200;
/// Questions use two seconds per blink, while retaining the current turn colour.
const QUESTION_HALF: u128 = 1000;
/// How long a tool-failure (orange) or edit-denial (red) flash shows after its timestamp.
const FLASH: Duration = Duration::from_millis(350);
/// The idle breath's period.
const BREATH: u128 = 2200;

/// The whole page for a phase. `age`/`now` as in the module docs.
pub fn frame(state: &State, age: Duration, now: SystemTime) -> Bitmap {
    let mut bm = Bitmap::blank();
    let t = state.trailer();
    let (activity, main, sub) = texts(state, now);

    let width = (activity.chars().count() as i32 * 6 - 1) * 2;
    let offset = activity_offset(width, age);
    text(&mut bm, 17 - offset, 3, &activity, 2);
    // The activity's viewport starts after the pulse. Text's ordinary clipping
    // handles the right edge; erase only this row's left gutter before the pulse.
    if offset > 0 {
        for x in 0..17 {
            for y in 3..17 {
                bm.set(x, y, false);
            }
        }
    }
    if beating(now) {
        bm.sprite(5, 4, HEART);
    } else {
        bm.sprite(4, 3, HEART_BIG);
    }
    if let Some(main) = main {
        text(&mut bm, 4, 24, &main, 1);
    }
    if t.agents > 0 {
        let label = if t.agents == 1 {
            "1 AGENT".to_string()
        } else {
            format!("{} AGENTS", t.agents)
        };
        let width = label.chars().count() as i32 * 6 - 1;
        text(&mut bm, W as i32 - width - 2, 24, &label, 1);
    }
    if let Some(sub) = sub {
        text(&mut bm, 4, 34, &sub, 1);
    }

    if inverted(state, age) {
        invert(&mut bm);
    }
    bm
}

/// The same 20 px/s travel and one-second end holds as the RA title marquee.
fn activity_offset(width: i32, age: Duration) -> i32 {
    let span = (width - (W as i32 - 17)).max(0) as u128;
    if span == 0 {
        return 0;
    }
    let travel = span * 50;
    let elapsed = age.as_millis() % (2000 + travel);
    (elapsed.saturating_sub(1000).min(travel) / 50) as i32
}

/// The backlight for a phase: the profile's `rest` while working, the alert colours
/// otherwise, with a tool-failure/denial flash overriding briefly on top of any of them.
pub fn colour(state: &State, age: Duration, now: SystemTime, rest: [u8; 3]) -> [u8; 3] {
    if matches!(state, State::Denied { .. }) {
        return RED;
    }
    let t = state.trailer();
    if flashing(t.deny_at, now) {
        return RED;
    }
    if flashing(t.fail_at, now) {
        return ORANGE;
    }
    match state {
        State::Turn { .. } | State::Waiting { .. } => rest,
        State::Ask {
            attention: super::Attention::Question { .. },
            started,
            ..
        } => {
            if started.is_some() {
                rest
            } else {
                GREEN
            }
        }
        State::Ask { .. } => YELLOW,
        State::Ready { .. } => GREEN,
        State::Denied { .. } => RED,
        State::Idle { .. } => breathe(GREEN, age),
    }
}

/// Activity word, the big stopwatch (if any), and the small sub-line (if any).
fn texts(state: &State, now: SystemTime) -> (String, Option<String>, Option<String>) {
    let last_sub = |t: super::Trailer| t.last.map(|d| format!("LAST {}", hms(d)));
    match state {
        State::Turn {
            activity,
            started,
            trailer,
        } => (
            activity.as_str().to_string(),
            Some(hmsc(now.duration_since(*started).unwrap_or_default())),
            last_sub(*trailer),
        ),
        State::Ask {
            attention, trailer, ..
        } => (
            match attention {
                super::Attention::Review => "REMOTE CHECK",
                super::Attention::Question { .. } => "QUESTION",
                super::Attention::Permission => "PERMISSIONS",
            }
            .to_string(),
            None,
            last_sub(*trailer),
        ),
        // A finished turn: the just-run length is the headline, frozen.
        State::Ready { trailer } | State::Idle { trailer } => {
            ("READY".to_string(), trailer.last.map(hmsc), None)
        }
        State::Denied { trailer } => ("DENIED".to_string(), trailer.last.map(hmsc), None),
        State::Waiting { .. } => ("AGENT".to_string(), None, Some("WAITING".to_string())),
    }
}

/// A duration as HH:MM:SS.CC (hundredths): the stopwatch's own format.
fn hmsc(d: Duration) -> String {
    let cs = d.as_millis() / 10;
    let (h, m, s, c) = (cs / 360_000, cs / 6_000 % 60, cs / 100 % 60, cs % 100);
    format!("{h:02}:{m:02}:{s:02}.{c:02}")
}

/// A duration as HH:MM:SS: the sub-line form, where hundredths would be noise.
fn hms(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Whether the pulse is contracted this instant (off the wall clock, so it never jumps).
fn beating(now: SystemTime) -> bool {
    let ms = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % BEAT;
    ms < CONTRACT
}

/// Whether the whole panel is inverted this frame: the permission blink repeats until he
/// answers; ready blinks twice and denied three times, then holds.
fn inverted(state: &State, age: Duration) -> bool {
    let ms = age.as_millis();
    match state {
        State::Ask {
            attention: super::Attention::Review,
            ..
        } => false,
        State::Ask {
            attention: super::Attention::Question { .. },
            ..
        } => (ms / QUESTION_HALF) % 2 == 1,
        State::Ask { .. } => (ms / BLINK_HALF) % 2 == 1,
        State::Ready { .. } | State::Denied { .. } => {
            (120..240).contains(&ms)
                || (420..540).contains(&ms)
                || (matches!(state, State::Denied { .. }) && (720..840).contains(&ms))
        }
        _ => false,
    }
}

/// Whether a flash timestamp is within its window of `now`.
fn flashing(at: Option<SystemTime>, now: SystemTime) -> bool {
    at.and_then(|t| now.duration_since(t).ok())
        .is_some_and(|d| d < FLASH)
}

/// Scale a colour by a smooth 0.45..1.0 breath over `BREATH`.
fn breathe(c: [u8; 3], age: Duration) -> [u8; 3] {
    let phase = (age.as_millis() % BREATH) as f32 / BREATH as f32;
    let f = 0.45 + 0.55 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos());
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
    ]
}

/// Invert every pixel: the "blink".
fn invert(bm: &mut Bitmap) {
    for y in 0..crate::lcd::H {
        for x in 0..crate::lcd::W {
            bm.set(x, y, !bm.get(x, y));
        }
    }
}

/// Mockup-only: dump the real renderer's frames for the key phases as PBMs plus a manifest
/// (name R G B inverted), so a review sheet shows the live page, not the mockup code.
/// Guarded by G13MAP_AGENT_DUMP; a no-op in the suite.
#[cfg(test)]
#[test]
fn dump_agent_frames() {
    let Some(dir) = std::env::var_os("G13MAP_AGENT_DUMP") else {
        return;
    };
    use crate::agent::{Activity, Trailer};
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    const WORKING: [u8; 3] = [0, 200, 255];
    let t0 = SystemTime::UNIX_EPOCH;
    let turn_trailer = Trailer {
        agents: 2,
        last: Some(Duration::from_millis(178_900)),
        ..Default::default()
    };
    let done = Trailer {
        agents: 2,
        last: Some(Duration::from_millis(174_890)),
        ..Default::default()
    };
    // (name, state, age, now) for each frame we want to see.
    let turn = State::Turn {
        activity: Activity::parse("EDITING"),
        started: t0,
        trailer: turn_trailer,
    };
    let search_fail = State::Turn {
        activity: Activity::parse("WEB_SEARCH"),
        started: t0,
        trailer: Trailer {
            fail_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(84)),
            ..turn_trailer
        },
    };
    let cases: Vec<(&str, State, Duration, SystemTime)> = vec![
        (
            "denied",
            State::Denied { trailer: done },
            Duration::from_secs(2),
            t0,
        ),
        (
            "denied-blink",
            State::Denied { trailer: done },
            Duration::from_millis(150),
            t0,
        ),
        (
            "long-start",
            State::Turn {
                activity: Activity::parse("ABCDEFGHIJKLMNOPQRST"),
                started: t0,
                trailer: turn_trailer,
            },
            Duration::ZERO,
            t0,
        ),
        (
            "long-moving",
            State::Turn {
                activity: Activity::parse("ABCDEFGHIJKLMNOPQRST"),
                started: t0,
                trailer: turn_trailer,
            },
            Duration::from_millis(3000),
            t0,
        ),
        (
            "long-end",
            State::Turn {
                activity: Activity::parse("ABCDEFGHIJKLMNOPQRST"),
                started: t0,
                trailer: turn_trailer,
            },
            Duration::from_millis(6000),
            t0,
        ),
        (
            "turn",
            turn,
            Duration::ZERO,
            t0 + Duration::from_millis(83_450),
        ),
        (
            "turn-fail",
            search_fail,
            Duration::ZERO,
            t0 + Duration::from_secs(84),
        ),
        (
            "ask",
            State::Ask {
                attention: crate::agent::Attention::Permission,
                started: None,
                trailer: turn_trailer,
            },
            Duration::ZERO,
            t0,
        ),
        (
            "ask-blink",
            State::Ask {
                attention: crate::agent::Attention::Permission,
                started: None,
                trailer: turn_trailer,
            },
            Duration::from_millis(BLINK_HALF as u64),
            t0,
        ),
        (
            "ready",
            State::Ready { trailer: done },
            Duration::from_millis(700),
            t0,
        ),
        (
            "idle",
            State::Idle { trailer: done },
            Duration::from_secs(1),
            t0,
        ),
        (
            "waiting",
            State::Waiting {
                trailer: Trailer::default(),
            },
            Duration::ZERO,
            t0,
        ),
    ];
    let mut manifest = String::new();
    for (name, state, age, now) in cases {
        // Freeze the pulse to the relaxed heart for a still (now at a non-beat instant).
        let now = now + Duration::from_millis(300);
        let bm = frame(&state, age, now);
        let [r, g, b] = colour(&state, age, now, WORKING);
        let inv = inverted(&state, age);
        let mut pbm = String::from("P1\n160 43\n");
        for y in 0..43 {
            for x in 0..160 {
                pbm.push_str(if bm.get(x, y) { "1 " } else { "0 " });
            }
            pbm.push('\n');
        }
        std::fs::write(dir.join(format!("{name}.pbm")), pbm).unwrap();
        manifest.push_str(&format!("{name} {r} {g} {b} {}\n", inv as u8));
    }
    std::fs::write(dir.join("frames.txt"), manifest).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Activity, Trailer};

    fn at(ms: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn stopwatch_formats_hundredths_by_hand() {
        // 1 h 2 m 3 s 45/100: 3723.45 s -> 372345 cs.
        assert_eq!(hmsc(Duration::from_millis(3_723_450)), "01:02:03.45");
        assert_eq!(hmsc(Duration::from_millis(0)), "00:00:00.00");
        // 50 fps steps the hundredths by 2; a 20 ms tick is exactly 2 cs.
        assert_eq!(hmsc(Duration::from_millis(20)), "00:00:00.02");
        assert_eq!(hms(Duration::from_millis(178_900)), "00:02:58");
    }

    #[test]
    fn question_blinks_slowly_in_the_current_turn_colour() {
        let working = State::Ask {
            attention: crate::agent::Attention::Question { asynchronous: true },
            started: Some(at(1000)),
            trailer: Trailer::default(),
        };
        assert_eq!(texts(&working, at(2000)).0, "QUESTION");
        assert_eq!(
            colour(&working, Duration::ZERO, at(2000), [0, 200, 255]),
            [0, 200, 255]
        );
        // One-second holds: a full blink each two seconds, versus permission's 400ms.
        for (ms, expected) in [
            (0, false),
            (200, false),
            (499, false),
            (500, false),
            (999, false),
            (1000, true),
            (1999, true),
            (2000, false),
        ] {
            assert_eq!(
                inverted(&working, Duration::from_millis(ms)),
                expected,
                "at {ms}ms"
            );
        }
        let waiting = State::Ask {
            attention: crate::agent::Attention::Question { asynchronous: true },
            started: None,
            trailer: Trailer {
                last: Some(Duration::from_millis(2500)),
                ..Default::default()
            },
        };
        assert_eq!(texts(&waiting, at(9999)).0, "QUESTION");
        assert_eq!(
            colour(&waiting, Duration::ZERO, at(9999), [0, 200, 255]),
            GREEN
        );
        assert!(inverted(&waiting, Duration::from_millis(1000)));
    }

    #[test]
    fn a_turn_ticks_and_a_finished_turn_freezes() {
        let turn = State::Turn {
            activity: Activity::parse("EDITING"),
            started: at(1_000),
            trailer: Trailer {
                agents: 2,
                last: Some(Duration::from_millis(178_900)),
                ..Default::default()
            },
        };
        // Elapsed is now - started: at 1.00 s + 1.23 s = 00:00:01.23.
        let (activity, main, sub) = texts(&turn, at(2_230));
        assert_eq!(activity, "EDITING");
        assert_eq!(main.as_deref(), Some("00:00:01.23"));
        assert_eq!(sub.as_deref(), Some("LAST 00:02:58"));
        // Ready shows the finished length frozen, no sub-line, and is green.
        let ready = State::Ready {
            trailer: Trailer {
                last: Some(Duration::from_millis(178_900)),
                ..Default::default()
            },
        };
        let (word, main, sub) = texts(&ready, at(9_999));
        assert_eq!(
            (word.as_str(), main.as_deref(), sub),
            ("READY", Some("00:02:58.90"), None)
        );
        assert_eq!(colour(&ready, Duration::ZERO, at(0), [1, 2, 3]), GREEN);
    }

    #[test]
    fn alerts_override_and_blinks_have_the_right_shape() {
        let base = Trailer::default();
        // A turn is the profile colour; a failure flash overrides it for 350 ms, then clears.
        let turn = State::Turn {
            activity: Activity::parse("X"),
            started: at(0),
            trailer: Trailer {
                fail_at: Some(at(10_000)),
                ..base
            },
        };
        assert_eq!(colour(&turn, Duration::ZERO, at(10_000), [9, 9, 9]), ORANGE);
        assert_eq!(colour(&turn, Duration::ZERO, at(10_300), [9, 9, 9]), ORANGE);
        assert_eq!(
            colour(&turn, Duration::ZERO, at(10_400), [9, 9, 9]),
            [9, 9, 9]
        );
        // Deny beats fail when both are live.
        let both = State::Turn {
            activity: Activity::parse("X"),
            started: at(0),
            trailer: Trailer {
                fail_at: Some(at(10_000)),
                deny_at: Some(at(10_000)),
                ..base
            },
        };
        assert_eq!(colour(&both, Duration::ZERO, at(10_100), [9, 9, 9]), RED);
        // Permission inverts half the time (repeating); ready inverts twice then holds.
        let ask = State::Ask {
            attention: crate::agent::Attention::Permission,
            started: None,
            trailer: base,
        };
        assert!(!inverted(&ask, Duration::from_millis(0)));
        assert!(inverted(&ask, Duration::from_millis(BLINK_HALF as u64)));
        let ready = State::Ready { trailer: base };
        assert!(inverted(&ready, Duration::from_millis(150))); // first blink
        assert!(inverted(&ready, Duration::from_millis(450))); // second blink
        assert!(!inverted(&ready, Duration::from_millis(700))); // then holds
    }

    #[test]
    fn the_whole_frame_lights_pixels_and_invert_is_total() {
        let turn = State::Turn {
            activity: Activity::parse("EDITING"),
            started: at(0),
            trailer: Trailer {
                agents: 2,
                last: Some(Duration::from_millis(1_000)),
                ..Default::default()
            },
        };
        let bm = frame(&turn, Duration::ZERO, at(1_234));
        let lit = (0..crate::lcd::H)
            .flat_map(|y| (0..crate::lcd::W).map(move |x| (x, y)))
            .filter(|&(x, y)| bm.get(x, y))
            .count();
        assert!(lit > 100, "the page should draw something: {lit}");
        // Inversion is the exact complement.
        let ask = State::Ask {
            attention: crate::agent::Attention::Permission,
            started: None,
            trailer: Trailer::default(),
        };
        // An Ask frame at a non-inverting moment, and its inverted half: exact complements.
        let up = frame(&ask, Duration::from_millis(0), at(0));
        let down = frame(&ask, Duration::from_millis(BLINK_HALF as u64), at(0));
        for y in 0..crate::lcd::H {
            for x in 0..crate::lcd::W {
                assert_eq!(
                    up.get(x, y),
                    !down.get(x, y),
                    "inversion complements at {x},{y}"
                );
            }
        }
    }
    #[test]
    fn long_activity_marquee_holds_each_end_and_keeps_the_pulse_clear() {
        let state = State::Turn {
            activity: Activity::parse("ABCDEFGHIJKLMNOPQRST"),
            started: at(0),
            trailer: Default::default(),
        };
        let draw = |ms| frame(&state, Duration::from_millis(ms), at(10_000));
        let start = draw(0);
        assert_eq!(start, draw(999));
        assert_eq!(start, draw(1000));
        assert_ne!(start, draw(1050)); // First pixel step after the one-second hold.
                                       // 20 glyphs: (20*6-1)*2=238px; viewport 160-17=143px.
                                       // 95px at 50ms/px => 4750ms travel, plus two 1000ms holds.
        let end = draw(5750);
        assert_ne!(start, end);
        assert_eq!(end, draw(6749));
        assert_eq!(start, draw(6750));
        for x in 0..17 {
            for y in 0..20 {
                assert_eq!(start.get(x, y), end.get(x, y), "pulse area {x},{y}");
            }
        }
    }

    #[test]
    fn remote_review_stays_steady_while_human_permission_blinks() {
        let review = State::Ask {
            attention: crate::agent::Attention::Review,
            started: None,
            trailer: Default::default(),
        };
        let ask = State::Ask {
            attention: crate::agent::Attention::Permission,
            started: None,
            trailer: Default::default(),
        };
        assert_eq!(texts(&review, at(0)).0, "REMOTE CHECK");
        assert_eq!(texts(&ask, at(0)).0, "PERMISSIONS");
        for ms in [0, 199, 200, 399, 400, 600, 1200] {
            assert!(!inverted(&review, Duration::from_millis(ms)));
            assert_eq!(
                colour(&review, Duration::from_millis(ms), at(0), [0, 82, 127]),
                YELLOW
            );
        }
        assert!(inverted(&ask, Duration::from_millis(200)));
    }

    #[test]
    fn denied_is_red_with_exactly_three_blinks_then_steady() {
        let denied = State::Denied {
            trailer: Trailer {
                last: Some(Duration::from_secs(1)),
                fail_at: Some(at(10_000)),
                ..Default::default()
            },
        };
        assert_eq!(texts(&denied, at(10_000)).0, "DENIED");
        let plain = frame(&denied, Duration::ZERO, at(10_000));
        for (ms, blink) in [
            (0, false),
            (119, false),
            (120, true),
            (239, true),
            (240, false),
            (419, false),
            (420, true),
            (539, true),
            (540, false),
            (719, false),
            (720, true),
            (839, true),
            (840, false),
            (1000, false),
            (60000, false),
        ] {
            let age = Duration::from_millis(ms);
            assert_eq!(inverted(&denied, age), blink, "at {ms} ms");
            assert_eq!(colour(&denied, age, at(10_000), [0, 82, 127]), RED);
            let drawn = frame(&denied, age, at(10_000));
            for y in 0..crate::lcd::H {
                for x in 0..crate::lcd::W {
                    assert_eq!(drawn.get(x, y), plain.get(x, y) ^ blink);
                }
            }
        }
    }
}
