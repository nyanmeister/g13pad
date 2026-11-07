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

// ---- Structured generation: valid profiles must survive to_text() -> parse() unchanged ----
//
// Mutation fuzzing above asks "does garbage panic?"; this asks "does the writer lose or
// alter anything the parser accepts?" (asked 2026-10-13, trial run 2026-11-07). The
// generator builds canonical profiles: no explicit KEY_RESERVED binds (absent means
// unbound), no legacy `# lcd health ...` names (parse migrates those on purpose), every
// `# run` paired with its `>RUN:` bind, extra lines that are not commands the parser
// claims. Anything else the writer produces must read back identical, with no notes.

fn gen_word(r: &mut Rng, alphabet: &[&str], max: usize) -> String {
    let n = 1 + r.next() % max;
    (0..n)
        .map(|_| alphabet[r.next() % alphabet.len()])
        .collect()
}

fn gen_profile(r: &mut Rng) -> Profile {
    use crate::profile::{Display, StickMode, UNBOUND};
    use crate::run::{Program, Run, When};
    const ACTION_ATOMS: &[&str] = &[
        "KEY_A",
        "KEY_LEFTCTRL+",
        "KEY_F13",
        "MLEFT",
        ">M1;",
        "!profile game",
        "KEY_1",
        " ",
        "KEY_SPACE",
        "+",
        "KEY_Ü",
        "KEY_RESERVED_NOT",
        ">RUN:G1;",
    ];
    const PROGRAM_ATOMS: &[&str] = &[
        "echo",
        "hold",
        "both",
        "released",
        "pressed",
        "~/bin/x",
        "--flag",
        "a\tb",
        "ü",
        "#",
        "\"quoted word\"",
        "x y",
        "/usr/bin/true",
        "-",
    ];
    const NAME_ATOMS: &[&str] = &[
        "health",
        " ",
        "cs2",
        "pic",
        "é",
        "#",
        "ra",
        "_",
        "my picture",
    ];
    const EXTRA_ATOMS: &[&str] = &[
        "font 5x8",
        "stickzone 0 0 1 1",
        "x",
        "é",
        " ",
        "#not",
        "bind",
        "rgb",
    ];
    let mut p = Profile::default();
    // Binds on real controls and on custom names.
    for _ in 0..r.next() % 8 {
        let key = if r.next() % 3 == 0 {
            gen_word(r, &["G", "X", "1", "9", "_", "cust"], 4)
        } else {
            crate::profile::CONTROLS[r.next() % crate::profile::CONTROLS.len()].to_string()
        };
        let action = gen_word(r, ACTION_ATOMS, 4).trim().to_string();
        if action.is_empty() || action == UNBOUND || crate::run::key_of_binding(&action).is_some() {
            continue;
        }
        p.binds.insert(key, action);
    }
    if r.next() % 2 == 0 {
        p.rgb = Some([r.next() as u8, r.next() as u8, r.next() as u8]);
    }
    if r.next() % 2 == 0 {
        p.leds = Some((r.next() % 16) as u8);
    }
    if r.next() % 2 == 0 {
        let name = gen_word(r, NAME_ATOMS, 3).trim().to_string();
        if !name.is_empty() && crate::meter::selects(Some(&name)).is_none() {
            p.lcd = Some(name);
        }
    }
    p.display = match r.next() % 11 {
        0 => Display::Health(crate::meter::Reader::Feed),
        1 => Display::Health(crate::meter::Reader::Cs2),
        2 => Display::Health(crate::meter::Reader::Dolphin),
        3 => Display::Mpv,
        4 => Display::Pressure,
        5 => Display::Audio,
        6 => Display::Mpd,
        7 => Display::Agent,
        8 => Display::Combat,
        9 => Display::Alert,
        _ => Display::Picture,
    };
    if r.next() % 3 == 0 {
        let mic = gen_word(r, &["alsa_input.", "usb-", "\"", "\\", "é", " ", "0"], 5);
        if !mic.is_empty() {
            p.audio_mic = Some(mic);
        }
    }
    p.stick = match r.next() % 3 {
        0 => Some(StickMode::Analog),
        1 => Some(StickMode::Keys),
        _ => None,
    };
    if r.next() % 3 == 0 {
        p.gamepad = Some(crate::gamepad::Mapping {
            stick: if r.next() % 2 == 0 {
                crate::gamepad::Stick::Left
            } else {
                crate::gamepad::Stick::Right
            },
            click: crate::gamepad::Click::ALL[r.next() % crate::gamepad::Click::ALL.len()],
            swap: r.next() % 2 == 0,
            invert_x: r.next() % 2 == 0,
            invert_y: r.next() % 2 == 0,
        });
    }
    // Programs on keys, each with the bind the watcher needs.
    for _ in 0..r.next() % 3 {
        let key = gen_word(r, &["G", "1", "2", "_", "R", "ab"], 3);
        let when = When::ALL[r.next() % When::ALL.len()];
        // Atoms become separate words, so a when-word can land first in a longer line.
        let n = 1 + r.next() % 4;
        let words: Vec<String> = (0..n)
            .map(|_| PROGRAM_ATOMS[r.next() % PROGRAM_ATOMS.len()])
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .map(String::from)
            .collect();
        if words.is_empty() {
            continue;
        }
        p.runs.insert(
            key.clone(),
            Run {
                when,
                program: Program { words },
            },
        );
        p.binds.insert(key.clone(), crate::run::binding(&key, when));
    }
    for _ in 0..r.next() % 3 {
        let line = gen_word(r, EXTRA_ATOMS, 3).trim().to_string();
        let cmd = line.split_whitespace().next().unwrap_or("");
        if line.is_empty()
            || line.starts_with('#')
            || ["bind", "rgb", "mod", "stickmode"].contains(&cmd)
        {
            continue;
        }
        p.extra.push(line);
    }
    p
}

#[test]
fn generated_profiles_round_trip_through_their_own_file() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 | 1)
        .unwrap_or(0x2026_1107);
    let mut rng = Rng(seed);
    let mut failures = Vec::new();
    for case in 0..3000 {
        let p = gen_profile(&mut rng);
        let text = p.to_text();
        let (back, notes) = Profile::parse(&text);
        if back != p || !notes.is_empty() {
            failures.push(format!(
                "case {case}: notes {notes:?}\n--- wrote\n{text}--- generated\n{p:#?}\n--- read back\n{back:#?}"
            ));
            if failures.len() >= 3 {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "seed {seed:#x}: {} of the first failures:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

// Focus rules: CLASS<TAB>PROFILE lines. Expected by hand before running: a class holding a
// tab, or starting with `#`, cannot survive this file format; both are excluded below and
// named here. Everything else must.
#[test]
fn generated_focus_rules_round_trip() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 | 1)
        .unwrap_or(0x2026_1107);
    let mut rng = Rng(seed);
    const CLASS_ATOMS: &[&str] = &[
        "kitty", "Steam", "é", " ", "\t", "#", "on", "off", "x y", "_",
    ];
    const PROFILE_ATOMS: &[&str] = &["default", "game", "é", " ", "#", "on", "7", "a b"];
    for case in 0..3000 {
        let mut rules = Rules {
            on: rng.next() % 2 == 0,
            rules: vec![],
        };
        for _ in 0..rng.next() % 5 {
            let class = gen_word(&mut rng, CLASS_ATOMS, 3).trim().to_string();
            let profile = gen_word(&mut rng, PROFILE_ATOMS, 3).trim().to_string();
            // Excluded, not fixed: a tab is the file's separator and `#` opens a comment,
            // and no X11 WM_CLASS has either. Left as the format's limit.
            if class.is_empty()
                || profile.is_empty()
                || class.contains('\t')
                || class.starts_with('#')
            {
                continue;
            }
            rules.set(&class, Some(&profile));
        }
        let back = Rules::parse(&rules.to_text());
        assert_eq!(
            back,
            rules,
            "seed {seed:#x} case {case}:\n{}",
            rules.to_text()
        );
    }
}

// The meter tuning file. Hand-expected failure before running: `log_file` with a space in
// the path (Steam library folders have them), since the parser takes the first word.
#[test]
fn generated_meter_tuning_round_trips() {
    use crate::meter::{Band, Flash, Sprite, Tuning, VehicleStyle};
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 | 1)
        .unwrap_or(0x2026_1107);
    let mut rng = Rng(seed);
    const NAME_ATOMS: &[&str] = &["dead", "low", "hold", "band", "é", "x", "_", "9"];
    const PATH_ATOMS: &[&str] = &["/home/u/", "Doom Eternal", "game.log", " ", "é", "x", "-"];
    for case in 0..3000 {
        let mut t = Tuning::default();
        let mut froms = std::collections::BTreeSet::from([0u32]);
        for _ in 0..rng.next() % 5 {
            froms.insert((rng.next() % 1000) as u32);
        }
        t.bands = froms
            .into_iter()
            .map(|from| Band {
                name: gen_word(&mut rng, NAME_ATOMS, 2),
                from,
                rgb: [rng.next() as u8, rng.next() as u8, rng.next() as u8],
            })
            .collect();
        let tenths =
            |r: &mut Rng, lo: u32, hi: u32| (lo + r.next() as u32 % (hi - lo + 1)) as f32 / 10.0;
        t.hold = tenths(&mut rng, 0, 600);
        t.calm = tenths(&mut rng, 2, 600);
        t.racing = tenths(&mut rng, 2, 600);
        t.swell = tenths(&mut rng, 1, 100);
        t.flash = [Flash::Off, Flash::Trace, Flash::Panel][rng.next() % 3];
        t.sprite = [Sprite::Heart, Sprite::V1][rng.next() % 2];
        t.vehicle = [VehicleStyle::Tread, VehicleStyle::Gear, VehicleStyle::Scan][rng.next() % 3];
        t.heart = [(rng.next() % 145) as i32, (rng.next() % 30) as i32];
        t.readout = [34 + (rng.next() % 127) as i32, (rng.next() % 30) as i32];
        t.cs2_port = (1 + rng.next() % 65535) as _;
        t.log_file = std::path::PathBuf::from(gen_word(&mut rng, PATH_ATOMS, 4).trim());
        if t.log_file.as_os_str().is_empty() {
            continue;
        }
        let back = Tuning::parse(&t.to_text());
        assert_eq!(back, t, "seed {seed:#x} case {case}:\n{}", t.to_text());
    }
}

#[test]
fn a_log_file_path_with_spaces_survives_the_tuning_file() {
    // Shrunk from the generated tuning case of 2026-11-07 (`log_file éDoom Eternal` read
    // back as `éDoom`). Expected values worked out by hand from the file format.
    use crate::meter::Tuning;
    let t = Tuning::parse("log_file /home/u/.steam/steamapps/common/Doom Eternal/game.log\n");
    assert_eq!(
        t.log_file.to_str(),
        Some("/home/u/.steam/steamapps/common/Doom Eternal/game.log")
    );
    let t = Tuning::parse("log_file   /tmp/a b.log   # trailing comment\n");
    assert_eq!(t.log_file.to_str(), Some("/tmp/a b.log"));
    let t = Tuning::parse("log_file\n");
    assert_eq!(
        t.log_file,
        Tuning::default().log_file,
        "an empty line keeps the default"
    );
}

// Three more writer/parser pairs. Hand-predicted before running: all three should round
// trip (f32 `{}` is shortest-round-trip, names are trimmed by both sides). A negative
// result is still the measurement.
#[test]
fn generated_text_options_modes_and_lcd_options_round_trip() {
    use crate::text_options::{Align, Options as TextOptions};
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 | 1)
        .unwrap_or(0x2026_1107);
    let mut rng = Rng(seed);
    const PATH_ATOMS: &[&str] = &[
        "/usr/share/fonts/",
        "DejaVu Sans",
        ".ttf",
        " ",
        "é",
        "x",
        "-",
    ];
    const NAME_ATOMS: &[&str] = &["default", "game", "é", " ", "#", "on", "7", "a b", "_"];
    for case in 0..3000 {
        let mut t = TextOptions::default();
        t.size = (60 + rng.next() % 661) as f32 / 10.0;
        t.wrap = rng.next() % 2 == 0;
        t.align = [Align::Left, Align::Center, Align::Right][rng.next() % 3];
        t.invert = rng.next() % 2 == 0;
        t.speed = (rng.next() % 121) as u16;
        if rng.next() % 2 == 0 {
            let font = gen_word(&mut rng, PATH_ATOMS, 4).trim().to_string();
            if !font.is_empty() {
                t.font = Some(font.into());
            }
        }
        assert_eq!(
            TextOptions::parse(&t.to_text()),
            t,
            "seed {seed:#x} case {case}:\n{}",
            t.to_text()
        );

        let mut m = Modes {
            on: rng.next() % 2 == 0,
            profiles: Default::default(),
        };
        for slot in m.profiles.iter_mut() {
            if rng.next() % 2 == 0 {
                let name = gen_word(&mut rng, NAME_ATOMS, 3).trim().to_string();
                if !name.is_empty() {
                    *slot = Some(name);
                }
            }
        }
        assert_eq!(
            Modes::parse(&m.to_text()),
            m,
            "seed {seed:#x} case {case}:\n{}",
            m.to_text()
        );

        let bits = |r: &mut Rng| f32::from_bits((r.next() as u32) & 0x7fff_ffff);
        let finite = |r: &mut Rng| loop {
            let v = bits(r);
            if v.is_finite() {
                return v;
            }
        };
        let mut o = Options {
            crop: None,
            level: rng.next() as u8,
            dither: (rng.next() % 1001) as f32 / 1000.0,
            invert: rng.next() % 2 == 0,
            background: rng.next() as u8,
        };
        if rng.next() % 2 == 0 {
            let (w, h) = loop {
                let (w, h) = (finite(&mut rng), finite(&mut rng));
                if w > 0.0 && h > 0.0 {
                    break (w, h);
                }
            };
            let sign = |r: &mut Rng, v: f32| if r.next() % 2 == 0 { v } else { -v };
            let x = finite(&mut rng);
            let y = finite(&mut rng);
            o.crop = Some(crate::lcd::Crop {
                x: sign(&mut rng, x),
                y: sign(&mut rng, y),
                w,
                h,
            });
        }
        assert_eq!(
            Options::parse(&o.to_text()),
            o,
            "seed {seed:#x} case {case}:\n{}",
            o.to_text()
        );
    }
}
