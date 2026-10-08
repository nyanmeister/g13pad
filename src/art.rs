// SPDX-License-Identifier: GPL-3.0-or-later
//! Looping pixel-art animations for the panel, drawn in code.
//!
//! Each scene is a closed loop of frames at one delay: the last frame repeats the first (the
//! tests check it) and is not kept. A scene is a pure function of the frame number; places
//! and timings come from the frame count and from hashes, never from clocks or carried
//! state. Every period inside a scene (a blink, a bubble's rise, a wave) divides the loop
//! length, so the loop closes without a jump. `g13map-anim` keeps scenes as LCD pictures and
//! the editor's Animations window shows and picks them.
use crate::draw::*;
use crate::lcd::{Animation, Bitmap, H, W};
use std::{f32::consts::TAU, time::Duration};

pub struct Scene {
    pub name: &'static str,
    pub about: &'static str,
    pub delay_ms: u32,
    /// The loop, closed: the last frame equals the first.
    closed: fn() -> Vec<Bitmap>,
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "skyline-rain",
        about: "Rain over a city at night; windows come and go",
        delay_ms: 50,
        closed: skyline_rain,
    },
    Scene {
        name: "starfield",
        about: "A ship through three layers of stars",
        delay_ms: 40,
        closed: starfield,
    },
    Scene {
        name: "pong",
        about: "Two paddles that never miss",
        delay_ms: 40,
        closed: pong,
    },
    Scene {
        name: "life",
        about: "Conway's Life: a garden of oscillators",
        delay_ms: 100,
        closed: life,
    },
    Scene {
        name: "waves",
        about: "A boat under the moon, a gull overhead",
        delay_ms: 50,
        closed: waves,
    },
    Scene {
        name: "cubes",
        about: "Two wireframe cubes turning against each other",
        delay_ms: 40,
        closed: cubes,
    },
    Scene {
        name: "tesseracts",
        about: "Two hypercubes turning through the fourth dimension",
        delay_ms: 40,
        closed: tesseracts,
    },
    Scene {
        name: "digital-rain",
        about: "Glyphs falling in columns",
        delay_ms: 40,
        closed: digital_rain,
    },
    Scene {
        name: "heartbeat",
        about: "A monitor trace, one beat per sweep",
        delay_ms: 40,
        closed: heartbeat,
    },
    Scene {
        name: "aquarium",
        about: "Fish, bubbles and swaying weed",
        delay_ms: 50,
        closed: aquarium,
    },
];

impl Scene {
    pub fn find(name: &str) -> Option<&'static Scene> {
        SCENES.iter().find(|s| s.name == name)
    }
    /// The frames with the loop closed: the last repeats the first.
    pub fn closed_loop(&self) -> Vec<Bitmap> {
        (self.closed)()
    }
    /// The frames as kept and played, each with the scene's delay.
    pub fn animation(&self) -> Animation {
        let mut frames = self.closed_loop();
        frames.pop();
        let delay = Duration::from_millis(self.delay_ms as u64);
        Animation {
            frames: frames.into_iter().map(|b| (delay, b)).collect(),
        }
    }
}

// ---- the scenes ----

fn skyline_rain() -> Vec<Bitmap> {
    const N: i32 = 120;
    // Buildings left to right: x, width, roof row.
    let mut buildings = Vec::new();
    let (mut x, mut i) = (-3i32, 0u32);
    while x < W as i32 {
        let w = 8 + (hash(1, i, 0) % 11) as i32;
        let roof = 8 + (hash(2, i, 0) % 22) as i32;
        buildings.push((x, w, roof));
        x += w + 1 + (hash(3, i, 0) % 3) as i32;
        i += 1;
    }
    frames(N as usize, |t, bm| {
        bm.line(0, 42, W as i32 - 1, 42);
        for (b, &(x, w, roof)) in buildings.iter().enumerate() {
            let b = b as u32;
            bm.line(x, roof, x, 41);
            bm.line(x + w - 1, roof, x + w - 1, 41);
            bm.line(x, roof, x + w - 1, roof);
            if roof < 14 {
                // An antenna with a slow beacon.
                let ax = x + w / 2;
                bm.line(ax, roof - 4, ax, roof - 1);
                if (t + (hash(6, b, 0) % 60) as i32) % 60 < 6 {
                    bm.plot(ax, roof - 5);
                }
            }
            // Windows, 2x2 on a 4-pixel grid. About one in ten changes, once per loop: more
            // read as flicker on the glass (asked 2026-10-07).
            let mut wy = roof + 3;
            while wy + 1 < 41 {
                let mut wx = x + 2;
                while wx + 1 < x + w - 1 {
                    let h = hash(4, b, (wx as u32) << 8 | wy as u32);
                    let u = t + (h >> 8) as i32;
                    let lit = match h % 20 {
                        0..=11 => true,
                        12..=17 => false,
                        18 => u % 120 < 100,
                        _ => u % 120 < 60,
                    };
                    if lit {
                        bm.sprite(wx, wy, &["##", "##"]);
                    }
                    wx += 4;
                }
                wy += 4;
            }
        }
        // Rain: streaks on cycles of 40, 30 and 24 frames, slanting with the wind, splashing
        // on the ground.
        for d in 0..80u32 {
            let h = hash(5, d, 0);
            let x = (h % 160) as i32;
            let k = 3 + (h >> 8) as i32 % 3;
            let u = (t + (h >> 16) as i32 % N) * k % N;
            let y = u * 48 / N - 4;
            for s in 0..k - 1 {
                bm.plot(x + s / 2, y - s);
            }
            if y >= 41 {
                bm.plot(x - 1, 41);
                bm.plot(x + 1, 41);
            }
        }
    })
}

fn starfield() -> Vec<Bitmap> {
    const N: i32 = 160;
    const SHIP: &[&str] = &[
        ".....#.........",
        "....###........",
        "....####.......",
        "#############..",
        "##oo###########",
        "#############..",
        "....####.......",
        "....###........",
        ".....#.........",
    ];
    frames(N as usize, |t, bm| {
        for (layer, &(count, speed, len)) in [(45, 1, 1), (22, 2, 2), (10, 4, 3)].iter().enumerate()
        {
            for i in 0..count {
                let h = hash(10 + layer as u32, i, 0);
                let (x0, y) = ((h % 160) as i32, ((h >> 8) % 43) as i32);
                let x = (x0 - t * speed).rem_euclid(160);
                if layer == 0 {
                    // The far stars twinkle.
                    let p = [16, 20, 32, 40][((h >> 16) % 4) as usize];
                    if (t + ((h >> 20) % p as u32) as i32) % p < 2 {
                        continue;
                    }
                }
                for s in 0..len {
                    bm.plot(x + s, y);
                }
            }
        }
        let (sx, sy) = (34, 17 + wave(t as f32 / 32.0, 3.0));
        bm.halo(sx, sy, 15, 9);
        bm.sprite(sx, sy, SHIP);
        let flame = if t % 2 == 0 { 4 } else { 2 };
        for s in 1..=flame {
            bm.plot(sx - s, sy + 4);
        }
        if flame > 2 {
            bm.plot(sx - 1, sy + 3);
            bm.plot(sx - 1, sy + 5);
        }
    })
}

fn pong() -> Vec<Bitmap> {
    // The ball crosses 152 columns at 2 per frame (period 152) and 38 rows at 1 per frame
    // (period 76): the loop is 152 frames.
    const N: i32 = 152;
    frames(N as usize, |t, bm| {
        bm.line(0, 0, 159, 0);
        bm.line(0, 42, 159, 42);
        for y in (2..41).step_by(4) {
            bm.plot(80, y);
            bm.plot(80, y + 1);
        }
        let bx = 2 + tri(2 * t, 152);
        let by = 1 + tri(t + 29, 38);
        bm.sprite(bx, by, &["###", "###", "###"]);
        // Paddles meet the ball on their side and drift back to the middle as it leaves.
        let near_left = 152 - (bx - 2);
        for (px, near) in [(0, near_left), (157, 152 - near_left)] {
            let centre = 21 + (by + 1 - 21) * near / 152;
            let top = (centre - 5).clamp(1, 30);
            for y in top..top + 11 {
                bm.plot(px, y);
                bm.plot(px + 1, y);
                bm.plot(px + 2, y);
            }
        }
    })
}

fn life() -> Vec<Bitmap> {
    // Oscillators of periods 2, 3, 8 and 15 placed apart: the garden repeats after 120
    // generations, which the loop test proves (a collision would break the period).
    const N: usize = 120;
    const PULSAR: &[&str] = &[
        "..###...###..",
        ".............",
        "#....#.#....#",
        "#....#.#....#",
        "#....#.#....#",
        "..###...###..",
        ".............",
        "..###...###..",
        "#....#.#....#",
        "#....#.#....#",
        "#....#.#....#",
        ".............",
        "..###...###..",
    ];
    const GALAXY: &[&str] = &[
        "######.##",
        "######.##",
        ".......##",
        "##.....##",
        "##.....##",
        "##.....##",
        "##.......",
        "##.######",
        "##.######",
    ];
    const EIGHT: &[&str] = &["###...", "###...", "###...", "...###", "...###", "...###"];
    const PENTADECATHLON: &[&str] = &["..#....#..", "##.####.##", "..#....#.."];
    const BEACON: &[&str] = &["##..", "##..", "..##", "..##"];
    const TOAD: &[&str] = &[".###", "###."];
    const CLOCK: &[&str] = &["..#.", "#.#.", ".#.#", ".#.."];
    const BLINKER: &[&str] = &["###"];
    let garden: &[(usize, usize, &[&str])] = &[
        (3, 15, PULSAR),
        (24, 17, GALAXY),
        (42, 18, EIGHT),
        (56, 21, PENTADECATHLON),
        (74, 19, BEACON),
        (82, 20, TOAD),
        (91, 20, BLINKER),
        (97, 19, CLOCK),
        (112, 15, PULSAR),
        (134, 17, GALAXY),
        (150, 20, TOAD),
        (10, 4, BLINKER),
        (30, 3, BEACON),
        (50, 4, TOAD),
        (70, 3, CLOCK),
        (90, 4, BLINKER),
        (110, 3, BEACON),
        (130, 4, TOAD),
        (150, 3, BLINKER),
        (15, 36, TOAD),
        (35, 35, CLOCK),
        (55, 36, BLINKER),
        (75, 35, BEACON),
        (95, 36, TOAD),
        (115, 35, CLOCK),
        (135, 36, BLINKER),
        (152, 35, BEACON),
    ];
    let mut grid = vec![false; W * H];
    for &(x, y, rows) in garden {
        // A pattern past the right edge would wrap onto the next row and go unnoticed.
        assert!(y + rows.len() <= H && rows.iter().all(|r| x + r.len() <= W));
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                if c == b'#' {
                    grid[(y + dy) * W + x + dx] = true;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(N + 1);
    for _ in 0..=N {
        let mut bm = Bitmap::blank();
        for (i, &on) in grid.iter().enumerate() {
            if on {
                bm.set(i % W, i / W, true);
            }
        }
        out.push(bm);
        grid = life_step(&grid);
    }
    out
}

fn life_step(g: &[bool]) -> Vec<bool> {
    let mut next = vec![false; W * H];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let mut n = 0;
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (xx, yy) = (x + dx, y + dy);
                if (0..W as i32).contains(&xx)
                    && (0..H as i32).contains(&yy)
                    && g[yy as usize * W + xx as usize]
                {
                    n += 1;
                }
            }
            let i = y as usize * W + x as usize;
            next[i] = matches!((g[i], n), (true, 2 | 3) | (false, 3));
        }
    }
    next
}

fn waves() -> Vec<Bitmap> {
    // The gull crosses once per loop and is away the other half; three fish jump out of
    // the front wave in its direction, once each (asked 2026-10-07).
    const N: i32 = 160;
    const BOAT: &[&str] = &[
        ".........#........",
        ".........##.......",
        ".........####.....",
        ".........######...",
        ".........########.",
        ".........######...",
        ".........####.....",
        ".........#........",
        "o#oooooo###ooooo#o",
        "o################o",
        "oo##############oo",
        "ooo############ooo",
    ];
    const GULL: [&[&str]; 2] = [&["#...#", ".#.#.", "..#.."], &["..#..", ".#.#.", "#...#"]];
    const FISH: &[&str] = &[".##.#", "#####", ".##.#"];
    let back = |x: i32, t: i32| 24 + wave(x as f32 / 40.0 + t as f32 / 80.0, 2.0);
    let front = |x: i32, t: i32| 32 + wave(x as f32 / 28.0 - t as f32 / 80.0 + 0.3, 3.0);
    frames(N as usize, |t, bm| {
        for i in 0..16u32 {
            let h = hash(30, i, 0);
            let (x, y) = ((h % 160) as i32, ((h >> 8) % 15) as i32);
            let p = [16, 20, 40][((h >> 16) % 3) as usize];
            if x < 126 && (t + ((h >> 20) % p as u32) as i32) % p >= 2 {
                bm.plot(x, y);
            }
        }
        bm.disc(138, 9, 6);
        bm.sprite(135, 6, &["oo", "o."]);
        bm.sprite(140, 11, &["o"]);
        if t < 70 {
            let gx = 160 - t * 165 / 70;
            bm.sprite(gx, 12, GULL[(t / 5 % 2) as usize]);
        }
        for x in 0..W as i32 - 1 {
            bm.line(x, back(x, t), x + 1, back(x + 1, t));
        }
        for x in 0..W as i32 {
            let yf = front(x, t);
            if x + 1 < W as i32 {
                bm.line(x, yf, x + 1, front(x + 1, t));
            }
            for y in yf + 2..H as i32 {
                if (x + 2 * y + t / 10) % 8 == 0 {
                    bm.plot(x, y);
                }
            }
        }
        bm.sprite(60, front(69, t) - 10, BOAT);
        // Each fish leaves the water at `x0`, arcs 18 px to the left over 18 frames and
        // splashes on the way out and back in.
        for (start, x0) in [(20, 125), (75, 40), (118, 150)] {
            let k = t - start;
            if !(0..=20).contains(&k) {
                continue;
            }
            let x = x0 - k.min(18);
            let surface = front(x + 2, t);
            if k <= 18 {
                let lift = (7.0 * (k as f32 / 18.0 * TAU / 2.0).sin()).round() as i32;
                bm.halo(x, surface - 1 - lift, 5, 3);
                bm.sprite(x, surface - 1 - lift, FISH);
            }
            if !(3..=16).contains(&k) {
                let sx = if k < 3 { x0 + 2 } else { x + 2 };
                bm.plot(sx - 2, front(sx, t) - 2 - k % 3);
                bm.plot(sx + 2, front(sx, t) - 2 - k % 3);
            }
        }
    })
}

fn cubes() -> Vec<Bitmap> {
    const N: i32 = 120;
    frames(N as usize, |t, bm| {
        let a = t as f32 / N as f32 * TAU;
        for (cx, dir) in [(45, 1.0f32), (115, -1.0)] {
            let (ya, xa) = (a * dir, 2.0 * a);
            let project = |i: i32| {
                let (x, y, z) = (
                    (i & 1) as f32 * 2.0 - 1.0,
                    (i >> 1 & 1) as f32 * 2.0 - 1.0,
                    (i >> 2 & 1) as f32 * 2.0 - 1.0,
                );
                let (x1, z1) = (x * ya.cos() + z * ya.sin(), -x * ya.sin() + z * ya.cos());
                let (y2, z2) = (y * xa.cos() - z1 * xa.sin(), y * xa.sin() + z1 * xa.cos());
                let s = 8.5 / (1.0 + 0.2 * z2);
                (
                    (cx as f32 + x1 * s).round() as i32,
                    (21.0 + y2 * s).round() as i32,
                )
            };
            for i in 0..8 {
                for bit in [1, 2, 4] {
                    let j = i ^ bit;
                    if i < j {
                        let (p, q) = (project(i), project(j));
                        bm.line(p.0, p.1, q.0, q.1);
                    }
                }
            }
        }
    })
}

fn tesseracts() -> Vec<Bitmap> {
    // Sixteen vertices (±1)^4 and the 32 edges between those differing in one coordinate.
    // Each turns in two planes at once (xw and yz: a double rotation, the one with no
    // fixed axis), then is projected to 3D by w and to the panel by z, after a fixed tilt
    // so the inner cube never sits square. The two turn opposite ways through w.
    const N: i32 = 160;
    frames(N as usize, |t, bm| {
        let a = t as f32 / N as f32 * TAU;
        for (cx, dir) in [(45, 1.0f32), (115, -1.0)] {
            let (p, q) = (a * dir, a);
            let project = |i: i32| {
                let c = |b: i32| (i >> b & 1) as f32 * 2.0 - 1.0;
                let (x, y, z, w) = (c(0), c(1), c(2), c(3));
                let (x, w) = (x * p.cos() - w * p.sin(), x * p.sin() + w * p.cos());
                let (y, z) = (y * q.cos() - z * q.sin(), y * q.sin() + z * q.cos());
                let k = 2.0 / (3.0 - w);
                let (x, y, z) = (x * k, y * k, z * k);
                let (tx, ty) = (0.45f32, 0.35f32);
                let (y, z) = (y * tx.cos() - z * tx.sin(), y * tx.sin() + z * tx.cos());
                let (x, z) = (x * ty.cos() + z * ty.sin(), -x * ty.sin() + z * ty.cos());
                let s = 11.0 / (1.0 + 0.18 * z);
                (
                    (cx as f32 + x * s).round() as i32,
                    (21.0 + y * s).round() as i32,
                )
            };
            for i in 0..16 {
                for bit in [1, 2, 4, 8] {
                    let j = i ^ bit;
                    if i < j {
                        let (p, q) = (project(i), project(j));
                        bm.line(p.0, p.1, q.0, q.1);
                    }
                }
            }
        }
    })
}

fn digital_rain() -> Vec<Bitmap> {
    // Each column is an endless ribbon of 20 (a third: 10) glyph slots, six pixels apart, scrolling
    // down one or two pixels a frame with no reset: the ribbon's period (60 or 120 px)
    // divides the distance scrolled per loop, so the loop closes mid-scroll. One run of
    // glyphs per ribbon (an inverted head and a trail of three to five), the rest dark.
    // Glyphs are fixed per column and slot and mutate every twelve frames. Earlier versions
    // restarted a drop before its tail had left the panel, which read as the column
    // vanishing (asked 2026-10-07, twice).
    const N: i32 = 240;
    frames(N as usize, |t, bm| {
        for c in 0..40 {
            let h = hash(20, c as u32, 0);
            if (h >> 24).is_multiple_of(4) {
                continue;
            }
            let speed = 1 + (h >> 20) as i32 % 2;
            let slots = if h.is_multiple_of(3) { 10 } else { 20 };
            let trail = 3 + (h >> 4) as i32 % 3;
            let run = (h >> 8) as i32 % slots;
            let u = t + (h >> 16) as i32 % N;
            let pos = (u * speed).rem_euclid(6 * slots);
            let x = 1 + 4 * c;
            for j in 0..slots {
                let k = (j - run).rem_euclid(slots);
                if k > trail {
                    continue;
                }
                let g = hash(22, c as u32, j as u32 * 64 + (u / 12 % (N / 12)) as u32) | 0x4210;
                let y0 = (pos - 6 * j).rem_euclid(6 * slots);
                for y in [y0, y0 - 6 * slots] {
                    if y <= -5 || y >= H as i32 {
                        continue;
                    }
                    for i in 0..15 {
                        let on = g >> i & 1 == 1;
                        if k == 0 {
                            bm.put(x + i % 3, y + i / 3, !on);
                        } else if on {
                            bm.plot(x + i % 3, y + i / 3);
                        }
                    }
                }
            }
        }
    })
}

fn heartbeat() -> Vec<Bitmap> {
    // One beat is 80 columns and the trace scrolls 2 per frame: 40 frames.
    const N: i32 = 40;
    const BEAT: i32 = 80;
    fn trace(u: i32) -> i32 {
        24 - ecg(u.rem_euclid(BEAT))
    }
    const HEART: &[&str] = &[
        ".##.##.", "#######", "#######", ".#####.", "..###..", "...#...",
    ];
    const BIG: &[&str] = &[
        ".###.###.",
        "#########",
        "#########",
        "#########",
        ".#######.",
        "..#####..",
        "...###...",
        "....#....",
    ];
    frames(N as usize, |t, bm| {
        for x in 0..W as i32 - 1 {
            let u = x + 2 * t;
            bm.line(x, trace(u), x + 1, trace(u + 1));
        }
        // The heart beats as the spike passes the right side.
        if (18..28).contains(&((2 * t + 150) % BEAT)) {
            bm.sprite(7, 3, BIG);
        } else {
            bm.sprite(8, 4, HEART);
        }
    })
}

fn aquarium() -> Vec<Bitmap> {
    // Three fish cross and spend seconds away before coming back (asked 2026-10-07); a
    // pufferfish stays put, bobbing, fluttering, blinking and blowing bubbles.
    const N: i32 = 240;
    const BIG: [&[&str]; 2] = [
        &[
            "........##.....",
            "#.......####...",
            "##....########.",
            "###.#########o#",
            "##....########.",
            "#.......####...",
        ],
        &[
            "........##.....",
            "........####...",
            "#.....########.",
            "###.#########o#",
            "#.....########.",
            "........####...",
        ],
    ];
    const MID: [&[&str]; 2] = [
        &[
            "......###...",
            "#...#######.",
            "##.#######o#",
            "#...#######.",
            "......###...",
        ],
        &[
            "......###...",
            "....#######.",
            "##.#######o#",
            "....#######.",
            "......###...",
        ],
    ];
    const SMALL: [&[&str]; 2] = [
        &["#..#####.", "##.####o#", "#..#####."],
        &["...#####.", "##.####o#", "...#####."],
    ];
    const PUFFER: [&[&str]; 2] = [
        &[
            "....#...#...#....",
            "......#####......",
            "..#.#########.#..",
            "...####o######...",
            "#..##oo#######.#.",
            "..##############.",
            "..o#############.",
            "..#######o######.",
            "#..###########.#.",
            "...###o#######...",
            "..#.#########.#..",
            "......#####......",
            "....#...#...#....",
        ],
        &[
            "....#...#...#....",
            "......#####......",
            "..#.#########.#..",
            "...####o######.#.",
            "#..##oo#######...",
            "..##############.",
            "..o#############.",
            "..#######o######.",
            "#..###########...",
            "...###o#######.#.",
            "..#.#########.#..",
            "......#####......",
            "....#...#...#....",
        ],
    ];
    // Where a fish of width `w` is on its crossing: on screen for `moving` frames of each
    // `period`, then away. Rightward from off the left edge; `left` mirrors it.
    let crossing = |u: i32, period: i32, moving: i32, w: i32, left: bool| -> Option<i32> {
        let m = u.rem_euclid(period);
        if m >= moving {
            return None;
        }
        let x = m * (W as i32 + w) / moving - w;
        Some(if left { W as i32 - w - x } else { x })
    };
    frames(N as usize, |t, bm| {
        let flap = (t / 8 % 2) as usize;
        bm.line(0, 42, 159, 42);
        for x in 0..W as i32 {
            let h = hash(40, x as u32, 0);
            if h.is_multiple_of(3) {
                bm.plot(x, 41);
            }
            if h.is_multiple_of(7) {
                bm.plot(x, 40);
            }
        }
        // Bubbles: each stream rises on a period dividing the loop, wobbling.
        for i in 0..7u32 {
            let h = hash(41, i, 0);
            let x = 8 + (h % 144) as i32;
            let len = [40, 60, 80, 120][((h >> 8) % 4) as usize];
            let y = 40 - (t + (h >> 16) as i32 % len) % len * 46 / len;
            let x = x + wave(t as f32 / 16.0 + i as f32 / 7.0, 1.0);
            if len >= 80 {
                bm.sprite(x - 1, y - 1, &[".#.", "#.#", ".#."]);
            } else {
                bm.plot(x, y);
            }
        }
        // Weed sways from the root up.
        for (x, height) in [(20, 14), (48, 9), (76, 17), (104, 11), (150, 15)] {
            for up in 0..height {
                let sway = wave(t as f32 / 40.0 + up as f32 / 24.0, up as f32 / 5.0);
                bm.plot(x + sway, 41 - up);
                if up % 4 == 2 {
                    bm.plot(x + sway - 1, 41 - up);
                    bm.plot(x + sway + 1, 41 - up);
                }
            }
        }
        // The pufferfish: a one-pixel bob, a tail flutter, a blink every 80 frames, and a
        // bubble from its mouth every 60.
        let (px, py) = (119, 15 + wave(t as f32 / 60.0, 1.0));
        bm.sprite(px, py, PUFFER[(t / 12 % 2) as usize]);
        if t % 80 < 4 {
            bm.sprite(px + 5, py + 4, &["##"]);
        }
        let age = (t + 30) % 60;
        if age < py + 6 {
            bm.plot(px + 1 + wave(age as f32 / 8.0, 1.0), py + 6 - age);
        }
        // The crossing fish, each in front of what it passes.
        if let Some(x) = crossing(t + 60, 240, 160, 15, false) {
            let y = 6 + wave(t as f32 / 40.0, 2.0);
            bm.halo(x, y, 15, 6);
            bm.sprite(x, y, BIG[flap]);
        }
        if let Some(x) = crossing(t + 110, 120, 80, 9, true) {
            let y = 20 + wave(t as f32 / 20.0 + 0.5, 1.0);
            bm.halo(x, y, 9, 3);
            bm.sprite_flipped(x, y, SMALL[flap]);
        }
        if let Some(x) = crossing(t + 200, 240, 100, 12, false) {
            let y = 30 + wave(t as f32 / 24.0, 2.0);
            bm.halo(x, y, 12, 5);
            bm.sprite(x, y, MID[flap]);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_closes_its_loop_and_moves() {
        for s in SCENES {
            let frames = s.closed_loop();
            assert!(frames.len() >= 3, "{}", s.name);
            assert_eq!(
                frames.first(),
                frames.last(),
                "{}: the loop does not close",
                s.name
            );
            assert_ne!(frames[0], frames[1], "{}: nothing moves", s.name);
            assert!(
                frames.iter().all(|f| f.lit() > 0),
                "{}: a dark frame",
                s.name
            );
            let a = s.animation();
            assert_eq!(a.frames.len(), frames.len() - 1);
            assert!(a.animated());
            assert!(s.delay_ms >= 20, "{}: faster than the driver", s.name);
            assert!(a.to_bytes().len() <= 256 << 10, "{}: over 256 KiB", s.name);
        }
    }

    #[test]
    fn names_are_distinct_valid_picture_names() {
        let mut names: Vec<_> = SCENES.iter().map(|s| s.name).collect();
        for n in &names {
            crate::validate_name(n).unwrap();
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SCENES.len());
    }

    #[test]
    fn life_keeps_its_population() {
        // No oscillator dies or explodes: the population stays within the garden's bounds.
        let frames = Scene::find("life").unwrap().closed_loop();
        let first = frames[0].lit();
        for (i, f) in frames.iter().enumerate() {
            let lit = f.lit();
            assert!(
                lit > first / 2 && lit < first * 3,
                "generation {i}: {lit} cells from {first}"
            );
        }
    }

    #[test]
    fn drawing_clips_and_mirrors() {
        let mut bm = Bitmap::blank();
        bm.line(-5, -5, 5, 5);
        bm.sprite(158, 41, &["###", "###"]);
        bm.sprite_flipped(0, 0, &["#..", "..."]);
        assert!(bm.get(0, 0) && bm.get(5, 5) && bm.get(159, 42) && bm.get(2, 0));
        bm.halo(1, 1, 3, 3);
        assert!(!bm.get(0, 0) && !bm.get(4, 4) && bm.get(5, 5));
    }
}
