// SPDX-License-Identifier: GPL-3.0-or-later
use super::render::*;
use super::source::*;
use super::transport::*;
use super::*;
#[cfg(test)]
mod behaviour {
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
                assert!(m.beat.lift.iter().all(|v| *v == 0));
                assert_eq!(m.beat.beat_age, None);
            }
            m.frame(parse("0 vehicle tank pilot 60", now, now).unwrap());
            assert_eq!(m.hold.dead, 0);
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
        assert!(m.beat.beat_age.is_none());
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
        assert_eq!(m.hold.dead, 0);
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
                m.beat.beat_now()
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
            ("v1", super::behaviour::v1_state(72)),
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
