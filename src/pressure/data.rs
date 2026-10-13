// SPDX-License-Identifier: GPL-3.0-or-later
//! PSI sampling and the shared System pressure LCD renderer; no device ownership.
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const W: usize = 160;
const H: usize = 43;
const BYTES: usize = 960;
const HISTORY: usize = 90;
const NAMES: [&str; 3] = ["cpu", "memory", "io"];
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pressure {
    avg10: f64,
    total: u64,
}
fn parse(text: &str) -> Option<Pressure> {
    let line = text
        .lines()
        .find(|line| line.split_whitespace().next() == Some("some"))?;
    let mut average = None;
    let mut total = None;
    for field in line.split_whitespace().skip(1) {
        let (key, value) = field.split_once('=')?;
        match key {
            "avg10" => {
                if average.is_some() {
                    return None;
                }
                average = Some(value.parse::<f64>().ok()?);
            }
            "total" => {
                if total.is_some() {
                    return None;
                }
                total = Some(value.parse::<u64>().ok()?);
            }
            _ => {}
        }
    }
    let avg10 = average?;
    (avg10.is_finite() && (0.0..=100.0).contains(&avg10)).then_some(Pressure {
        avg10,
        total: total?,
    })
}
fn bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000 | 0o400000)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("expected regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::other("file exceeds size limit"));
    }
    Ok(bytes)
}
fn read_pressure(root: &Path) -> [Option<Pressure>; 3] {
    NAMES.map(|name| {
        bounded(&root.join(name), 4096)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| parse(&s))
    })
}
fn recent(before: Pressure, after: Pressure, elapsed: Duration) -> Option<f64> {
    let delta = after.total.checked_sub(before.total)?;
    let micros = elapsed.as_secs_f64() * 1_000_000.0;
    if micros <= 0.0 || !micros.is_finite() {
        return None;
    }
    // PSI counters are updated asynchronously; a short interval can exceed wall
    // time slightly. Bound the plotted percentage, not the cumulative counter.
    Some((delta as f64 * 100.0 / micros).clamp(0.0, 100.0))
}
fn advance(history: &mut [VecDeque<Option<f64>>; 3], buckets: u64, samples: [Option<f64>; 3]) {
    for (row, values) in history.iter_mut().enumerate() {
        let retained = buckets.min(HISTORY as u64);
        for slot in 0..retained {
            // A long pause is missing time, never compressed into one column.
            values.push_back(if slot + 1 == retained {
                samples[row]
            } else {
                None
            });
            if values.len() > HISTORY {
                values.pop_front();
            }
        }
    }
}

type Frame = [u8; BYTES];
fn pixel(frame: &mut Frame, x: usize, y: usize) {
    if x < W && y < H {
        frame[x + y / 8 * W] |= 1 << (y % 8);
    }
}
// The same 5x7 letter/digit designs used by the meter, restricted to this page.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 17, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}
fn text(frame: &mut Frame, x: usize, y: usize, value: &str) {
    for (i, c) in value.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    pixel(frame, x + i * 6 + col, y + row);
                }
            }
        }
    }
}
fn scale(history: &[VecDeque<Option<f64>>; 3]) -> u32 {
    let peak = history
        .iter()
        .flatten()
        .flatten()
        .copied()
        .fold(0.0, f64::max);
    [10, 25, 50, 100]
        .into_iter()
        .find(|s| peak <= f64::from(*s))
        .unwrap_or(100)
}
fn render(
    current: [Option<Pressure>; 3],
    history: &[VecDeque<Option<f64>>; 3],
    synthetic: bool,
) -> Frame {
    let mut frame = [0; BYTES];
    let ceiling = scale(history);
    text(
        &mut frame,
        0,
        0,
        &format!(
            "{}10S 90S/{}%",
            if synthetic { "DEMO" } else { "STALL" },
            ceiling
        ),
    );
    for (row, label) in ["CPU", "MEM", "I/O"].into_iter().enumerate() {
        let y = 11 + row * 11;
        text(&mut frame, 0, y, label);
        let value = current[row].map_or_else(|| "--.-%".into(), |p| format!("{:5.1}%", p.avg10));
        text(&mut frame, 24, y, &value);
        // Ninety one-second intervals, fixed time axis; one shared labelled scale.
        for (i, value) in history[row].iter().enumerate() {
            let x = 70 + HISTORY - history[row].len() + i;
            if let Some(value) = value {
                let height = (value / f64::from(ceiling) * 7.0).ceil().clamp(0.0, 7.0) as usize;
                pixel(&mut frame, x, y + 7);
                for h in 0..height {
                    pixel(&mut frame, x, y + 6 - h);
                }
            }
        }
    }
    frame
}
/// One-second samples continue while the editor temporarily owns the LCD.
pub struct Sampler {
    root: PathBuf,
    started: Instant,
    previous_at: Instant,
    previous_bucket: u64,
    previous: [Option<Pressure>; 3],
    current: [Option<Pressure>; 3],
    history: [VecDeque<Option<f64>>; 3],
}
impl Sampler {
    pub fn new(root: PathBuf, now: Instant) -> Self {
        let current = read_pressure(&root);
        Self {
            root,
            started: now,
            previous_at: now,
            previous_bucket: 0,
            previous: current,
            current,
            history: std::array::from_fn(|_| VecDeque::with_capacity(HISTORY)),
        }
    }
    pub fn sample(&mut self, now: Instant) {
        self.current = read_pressure(&self.root);
        let bucket = now.duration_since(self.started).as_secs();
        if bucket <= self.previous_bucket {
            return;
        }
        let elapsed = now.duration_since(self.previous_at);
        let slots = bucket - self.previous_bucket;
        let samples = std::array::from_fn(|row| {
            if slots != 1 || elapsed > Duration::from_millis(1500) {
                None
            } else {
                self.previous[row]
                    .zip(self.current[row])
                    .and_then(|(a, b)| recent(a, b, elapsed))
            }
        });
        advance(&mut self.history, slots, samples);
        self.previous = self.current;
        self.previous_at = now;
        self.previous_bucket = bucket;
    }
    pub fn frame(&self) -> [u8; BYTES] {
        render(self.current, &self.history, false)
    }
}
/// Clearly marked illustrative data for hardware-free layout previews.
pub fn example_frame() -> [u8; BYTES] {
    let history = std::array::from_fn(|row| {
        (0..HISTORY)
            .map(|i| Some(((i as f64 / 7.0 + row as f64).sin() + 1.0) * [4.0, 2.0, 1.0][row]))
            .collect()
    });
    render(
        [
            Some(Pressure {
                avg10: 6.5,
                total: 0,
            }),
            Some(Pressure {
                avg10: 1.2,
                total: 0,
            }),
            Some(Pressure {
                avg10: 0.3,
                total: 0,
            }),
        ],
        &history,
        true,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_some_not_full_and_rejects_bad_or_duplicate_fields() {
        assert_eq!(
            parse("full avg10=99 total=5\nsome total=100 avg300=0 avg10=2.5 avg60=0\n"),
            Some(Pressure {
                avg10: 2.5,
                total: 100
            })
        );
        for line in [
            "some avg10=NaN total=1",
            "some avg10=101 total=1",
            "some avg10=-1 total=1",
            "full avg10=1 total=1",
            "some avg10=1",
            "some avg10=1 total=2 total=3",
            "some avg10=1 avg10=2 total=3",
        ] {
            assert!(parse(line).is_none(), "{line}");
        }
    }
    #[test]
    fn rate_uses_elapsed_time_and_reset_is_unknown() {
        let a = Pressure {
            avg10: 0.0,
            total: 100,
        };
        let b = Pressure {
            avg10: 0.0,
            total: 200100,
        };
        assert_eq!(recent(a, b, Duration::from_secs(2)), Some(10.0));
        assert_eq!(recent(b, a, Duration::from_secs(1)), None);
        assert_eq!(recent(a, b, Duration::ZERO), None);
        assert_eq!(recent(a, b, Duration::from_millis(100)), Some(100.0));
    }
    #[test]
    fn scale_is_shared_bounded_and_missing_samples_do_not_become_zero() {
        let mut h = std::array::from_fn(|_| VecDeque::new());
        assert_eq!(scale(&h), 10);
        h[2].push_back(Some(24.0));
        assert_eq!(scale(&h), 25);
        h[0].push_back(Some(99.0));
        assert_eq!(scale(&h), 100);
        let missing = render(
            [None; 3],
            &std::array::from_fn(|_| VecDeque::from([None])),
            false,
        );
        let zero = render(
            [None; 3],
            &std::array::from_fn(|_| VecDeque::from([Some(0.0)])),
            false,
        );
        assert_ne!(missing, zero);
    }
    #[test]
    fn renderer_never_uses_unused_lcd_rows_and_marks_synthetic() {
        let h = std::array::from_fn(|_| VecDeque::from(vec![Some(100.0); HISTORY]));
        let real = render(
            [Some(Pressure {
                avg10: 100.0,
                total: 0,
            }); 3],
            &h,
            false,
        );
        let demo = render(
            [Some(Pressure {
                avg10: 100.0,
                total: 0,
            }); 3],
            &h,
            true,
        );
        assert_ne!(real, demo);
        assert!(real[800..].iter().all(|byte| byte & 0xf8 == 0));
    }
    #[test]
    fn delayed_sampling_preserves_missing_seconds_and_discards_expired_history() {
        let mut h = std::array::from_fn(|_| VecDeque::from([Some(2.0)]));
        advance(&mut h, 3, [None; 3]);
        assert_eq!(h[0], VecDeque::from([Some(2.0), None, None, None]));
        advance(&mut h, 1, [Some(4.0); 3]);
        assert_eq!(h[0].back(), Some(&Some(4.0)));
        advance(&mut h, 200, [None; 3]);
        assert_eq!(h[0].len(), HISTORY);
        assert!(h.iter().flatten().all(Option::is_none));
    }
}
