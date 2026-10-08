// SPDX-License-Identifier: GPL-3.0-or-later
//! Reproducible mutation fuzzing without a nightly compiler or extra dependencies.
//! No device, display, config file, or process environment is touched.
use crate::{
    daemon,
    focus::Rules,
    lcd::{Animation, Options},
    modes::Modes,
    profile::Profile,
};
use std::time::Duration;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
    fn mutate(&mut self, seed: &[u8]) -> Vec<u8> {
        let mut b = seed.to_vec();
        for _ in 0..1 + self.next() % 16 {
            let i = self.next() % (b.len() + 1);
            match self.next() % 4 {
                0 if i < b.len() => {
                    b.remove(i);
                }
                1 if i < b.len() => b[i] ^= 1 << (self.next() % 8),
                2 => {
                    b.truncate(i);
                }
                _ => b.insert(i, self.next() as u8),
            }
        }
        b
    }
}

#[test]
#[ignore = "run explicitly: cargo test --offline fuzz_tests -- --ignored --nocapture"]
fn mutation_fuzz_parsers_and_command_framing() {
    let seeds: &[&[u8]] = &[
        b"bind G10 KEY_LEFTCTRL\nbind G15 KEY_LEFTSHIFT\nrgb 31 0 127\nmod 15\n",
        b"bind G1 >M1;\nbind G2 !profile game\nbind G3 KEY_A KEY_B\n",
        b"on\nsteam_app_548430\tdeeprock\nkitty shell\n0 default\n1 game\n7 alt\n",
        b"crop -20 1.5 160 43\nlevel 128\ndither 0.5\ninvert 1\n",
        b"crop NaN inf 1e38 1e-38\ndither NaN\nlevel inf\n",
        b"# lcd my picture\nfont 5x8\n\t\r\n\0",
        b"bind\nbind G1\nmod\nrgb 1 nope 2 3\n",
        b"# stick keys\n# stick analog\n# stick current\n# stick invalid\n",
        b"# gamepad right r3 1 1 0\nstick right\nclick none\nswap 0\ninvert_x 1\ninvert_y 0\n",
        b"background 73\nbackground -200\nbackground NaN\nbackground inf\n",
        b"size 18.5\nwrap 1\nalign right\ninvert 1\nspeed 72\nfont /tmp/a font.ttf\n",
        b"size NaN\nsize inf\nspeed 999\nalign invalid\n",
    ];
    let mut rng = Rng(0x13_2026_0929);
    for i in 0..100_000 {
        let bytes = rng.mutate(seeds[i % seeds.len()]);
        let text = String::from_utf8_lossy(&bytes);
        let (profile, _) = Profile::parse(&text);
        if let Ok(mapping) = crate::gamepad::Mapping::parse(&text) {
            assert_eq!(
                crate::gamepad::Mapping::parse(&mapping.to_text()).unwrap(),
                mapping
            );
            assert!(mapping.absmap().starts_with("ABS_X="));
            assert!(mapping.axismap().split(',').count() == 2);
        }
        if let Ok(mapping) = crate::gamepad::Mapping::from_compact(&text) {
            assert_eq!(
                crate::gamepad::Mapping::from_compact(&mapping.compact()).unwrap(),
                mapping
            );
        }
        assert_eq!(
            Profile::parse(&profile.to_text()).0,
            profile,
            "case {i}: {text:?}"
        );
        let modes = Modes::parse(&text);
        assert_eq!(Modes::parse(&modes.to_text()), modes, "case {i}");
        let rules = Rules::parse(&text);
        assert_eq!(Rules::parse(&rules.to_text()), rules, "case {i}: {text:?}");
        let options = Options::parse(&text);
        assert!(options.dither.is_finite());
        assert_eq!(Options::parse(&options.to_text()), options);
        let text_options = crate::text_options::Options::parse(&text);
        assert!(text_options.validate().is_ok());
        assert_eq!(
            crate::text_options::Options::parse(&text_options.to_text()),
            text_options
        );
        if let Ok(chunks) = daemon::command_chunks(&text) {
            for chunk in &chunks {
                assert!(chunk.len() <= 4096 && chunk.len() != 960);
                assert!(chunk.ends_with(b"\n") && !chunk.contains(&0));
            }
            let framed = String::from_utf8(chunks.concat()).unwrap();
            // Padding adds only empty lines, which the daemon ignores.
            assert_eq!(
                framed
                    .split(['\r', '\n'])
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>(),
                text.split(['\r', '\n'])
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            );
        } else {
            assert!(
                text.contains('\0')
                    || text
                        .split_inclusive('\n')
                        .any(|s| s.len() + usize::from(!s.ends_with('\n')) > 4096)
            );
        }
    }
    // Boundary lengths, multi-write profiles, NULs and a too-long single line.
    for n in [
        0, 1, 958, 959, 960, 961, 4094, 4095, 4096, 4097, 8192, 65536,
    ] {
        let text = "x".repeat(n);
        assert_eq!(daemon::command_chunks(&text).is_ok(), n < 4096);
        let many = "bind G1 KEY_A\n".repeat(n / 14);
        let chunks = daemon::command_chunks(&many).unwrap();
        assert!(chunks.iter().all(|c| c.len() <= 4096 && c.len() != 960));
        assert_eq!(String::from_utf8(chunks.concat()).unwrap(), many);
    }
    println!("100,000 seeded mutation cases: profile, modes, focus, LCD options, FIFO framing");
}

#[test]
#[ignore = "run explicitly with the other mutation fuzz test"]
fn mutation_fuzz_animation_records() {
    let mut rng = Rng(0x13_a11ce);
    for i in 0..100_000 {
        let n = match i % 3 {
            0 => 964,
            1 => 1928,
            _ => rng.next() % 4096,
        };
        let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let parsed = Animation::from_bytes(&bytes);
        assert_eq!(parsed.is_ok(), n > 0 && n.is_multiple_of(964));
        if let Ok(animation) = parsed {
            assert_eq!(animation.to_bytes(), bytes);
            let (frame, wait) = animation.due(Duration::from_nanos(rng.next() as u64));
            assert!(frame < animation.frames.len() && !wait.is_zero());
            assert_eq!(animation.first().0.len(), 960);
        }
    }
    println!("100,000 animation record/timing mutation cases");
}

#[test]
#[ignore = "run explicitly with the other mutation fuzz tests"]
fn mutation_fuzz_image_rendering_and_marquee_text() {
    let source = crate::lcd::Source::fuzz_fixture();
    let mut rng = Rng(0x13_2026_0930);
    let options_seeds: &[&[u8]] = &[
        b"crop -12 0 50 30\nbackground 73\nlevel 128\ndither 0.6\ninvert 1\n",
        b"crop -1e38 -1e38 1e38 1e38\nbackground 255\n",
        b"crop 1e-38 1e-38 1e-38 1e-38\nbackground 0\n",
    ];
    for i in 0..5000 {
        let bytes = rng.mutate(options_seeds[i % options_seeds.len()]);
        let o = Options::parse(&String::from_utf8_lossy(&bytes));
        let bm = source.render(&o);
        let flipped = source.render(&Options {
            invert: !o.invert,
            ..o
        });
        for y in 0..crate::lcd::H {
            for x in 0..crate::lcd::W {
                assert_ne!(bm.get(x, y), flipped.get(x, y), "case {i}");
            }
        }
        assert_eq!(bm.0.len(), crate::lcd::BYTES);
        assert!(bm.0[800..].iter().all(|b| b & 0xf8 == 0));
    }
    for i in 0..1000 {
        let bytes = rng.mutate(if i % 2 == 0 {
            b"Error: profile unavailable"
        } else {
            b"G13"
        });
        let text = String::from_utf8_lossy(&bytes);
        let error = crate::overlay::error_frame(&text).unwrap();
        assert_eq!(error.len(), crate::lcd::BYTES);
        assert!(error[800..].iter().all(|b| b & 0xf8 == 0));
        let options = crate::text_options::Options {
            size: (6 + rng.next() % 67) as f32,
            wrap: rng.next().is_multiple_of(2),
            align: match rng.next() % 3 {
                0 => crate::text_options::Align::Left,
                1 => crate::text_options::Align::Center,
                _ => crate::text_options::Align::Right,
            },
            invert: rng.next().is_multiple_of(2),
            speed: (rng.next() % 121) as u16,
            ..crate::text_options::Options::default()
        };
        if let Ok(a) = crate::marquee::render_with(&text, &options) {
            assert!(!a.frames.is_empty() && a.frames.len() < 3000);
            assert_eq!(
                Animation::from_bytes(&a.to_bytes()).unwrap().to_bytes(),
                a.to_bytes()
            );
            assert!(a.frames.iter().all(|(_, bm)| bm.0.len() == 960));
        }
    }
    println!("5,000 crop/background/dither rendering cases and 1,000 text/settings mutations");
}
