// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
const DOLPHIN_PREFIX: &str = "00:01:500 Core/AchievementManager.cpp:79 I[RetroAchievements]: ";
fn dolphin_record(message: &str) -> String {
    format!("{DOLPHIN_PREFIX}{message}\n")
}

#[test]
fn dolphin_sets_require_a_known_normal_session_and_sum_distinct_set_ids() {
    let mut m = Model::default();
    for line in [
        "Encore mode disabled",
        "Identified game: 15543 \"Skies of Arcadia Legends\" (hash)",
        "Game 15543 loaded, hardcore enabled",
        "Set 11576: 142/180 achievements active, 0 leaderboards",
    ] {
        m.ingest(dolphin_event(&dolphin_record(line)).unwrap(), false);
    }
    assert_eq!(
        (m.game, m.earned, m.total, m.hardcore),
        (Some(15543), Some(38), Some(180), Some(true))
    );
    m.ingest(
        dolphin_event(&dolphin_record(
            "Set 555: 2/10 achievements active, 0 leaderboards",
        ))
        .unwrap(),
        false,
    );
    assert_eq!((m.earned, m.total), (Some(46), Some(190)));
    // Repeated summaries replace that set rather than inflating totals.
    m.ingest(
        dolphin_event(&dolphin_record(
            "Set 555: 2/10 achievements active, 0 leaderboards",
        ))
        .unwrap(),
        false,
    );
    assert_eq!(m.total, Some(190));
    m.ingest(
        dolphin_event(&dolphin_record(
            "Set 555: 2/10 achievements active (1 unsupported), 0 leaderboards",
        ))
        .unwrap(),
        false,
    );
    assert_eq!((m.earned, m.total), (None, Some(190)));
    assert_eq!(
        dolphin_event(&dolphin_record(
            "Set 1: 11/10 achievements active, 0 leaderboards"
        )),
        None
    );
    m.ingest(Event::Encore(true), false);
    m.ingest(Event::Assets(555, 2, 10, true), false);
    assert_eq!(m.earned, None);
    m.ingest(Event::Mode(false), false);
    assert!(m.sets.is_empty());
    assert_eq!(m.earned, None);
}

#[test]
fn dolphin_tail_excludes_old_sessions_and_reconstructs_confirmed_gain() {
    let _sandbox = crate::test_support::Sandbox::new("ra-dolphin-tail");
    let path = crate::state_dir().join("dolphin.log");
    fs::create_dir_all(crate::state_dir()).unwrap();
    fs::write(&path, [
            "59:50:000 Core/AchievementManager.cpp:96 I[RetroAchievements]: Achievement Manager Initialized\n".into(),
            dolphin_record("Encore mode disabled"),
            dolphin_record("Identified game: 999 \"OLD\" (hash)"),
            dolphin_record("Game 999 loaded, hardcore disabled"),
            dolphin_record("Set 99: 0/10 achievements active, 0 leaderboards"),
        ].concat()).unwrap();
    let mut tail = Tail::default();
    let mut m = Model::default();
    tail.poll(&path, &mut m, Source::Dolphin, Some(1000))
        .unwrap();
    assert_eq!(m.game, None); // Old crash log isn't the live process's session.
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    for s in [
        "Achievement Manager Initialized",
        "Encore mode disabled",
        "Identified game: 15543 \"Skies\" (hash)",
        "Game 15543 loaded, hardcore enabled",
        "Set 11576: 142/180 achievements active, 0 leaderboards",
    ] {
        write!(file, "{}", dolphin_record(s)).unwrap();
    }
    tail.poll(&path, &mut m, Source::Dolphin, Some(1000))
        .unwrap();
    assert_eq!((m.earned, m.total), (Some(38), Some(180)));
    write!(
        file,
        "{}{}",
        dolphin_record("Awarding achievement 8: New trophy"),
        dolphin_record("Achievement 8 awarded, new score: 4406")
    )
    .unwrap();
    tail.poll(&path, &mut m, Source::Dolphin, Some(1000))
        .unwrap();
    assert_eq!((m.earned, m.gained, m.queue.len()), (Some(39), 1, 1));
    let mut restarted = Model::default();
    Tail::default()
        .poll(&path, &mut restarted, Source::Dolphin, Some(1000))
        .unwrap();
    assert_eq!((restarted.earned, restarted.gained), (Some(39), 1));
    assert!(restarted.queue.is_empty());
    write!(file, "{}", dolphin_record("Game closed.")).unwrap();
    tail.poll(&path, &mut m, Source::Dolphin, Some(1000))
        .unwrap();
    assert_eq!(m.game, None);
    // Reject a wholly stale file, and hour rollover is a valid new initialization.
    Tail::default()
        .poll(&path, &mut restarted, Source::Dolphin, Some(u64::MAX))
        .unwrap();
    assert_eq!(restarted.game, None);
    assert!(dolphin_init_matches("00:01:500 initialization", 3599000));
    assert!(!dolphin_init_matches("01:59:999 initialization", 3599000));
}

#[test]
fn same_game_reload_clears_active_notification() {
    let now = Instant::now();
    let mut m = Model::default();
    let mut d = Display::default();
    m.ingest(Event::Identity(1, "GAME".into()), false);
    m.ingest(Event::Unlock(5, "TROPHY".into()), true);
    d.frame(&mut m, now);
    assert!(d.card.is_some());
    m.ingest(Event::Unload, false);
    m.ingest(Event::Identity(1, "GAME".into()), false);
    d.frame(&mut m, now);
    assert!(d.card.is_none());
}

#[test]
fn dolphin_native_discovery_refuses_simultaneous_writers_and_read_only_logs() {
    use std::os::unix::fs::symlink;
    let _sandbox = crate::test_support::Sandbox::new("ra-dolphin-native");
    let proc_root = crate::state_dir().join("proc");
    fs::create_dir_all(&proc_root).unwrap();
    fs::write(proc_root.join("stat"), "btime 0\n").unwrap();
    let log = crate::state_dir().join("dolphin.log");
    fs::write(&log, dolphin_record("Achievement Manager Initialized")).unwrap();
    for pid in [123, 124] {
        let process = proc_root.join(pid.to_string());
        fs::create_dir_all(process.join("fd")).unwrap();
        fs::create_dir_all(process.join("fdinfo")).unwrap();
        fs::write(process.join("comm"), "dolphin-emu\n").unwrap();
        let mut fields = vec!["0"; 20];
        fields[0] = "S";
        fields[19] = if pid == 123 { "100" } else { "200" };
        fs::write(
            process.join("stat"),
            format!("{pid} (dolphin-emu) {}", fields.join(" ")),
        )
        .unwrap();
        symlink(&log, process.join("fd/3")).unwrap();
        fs::write(process.join("fdinfo/3"), "flags:\t0102001\n").unwrap();
        if pid == 123 {
            let s = native_session(&proc_root, Source::Dolphin).unwrap();
            assert_eq!(s.pid, 123);
            assert!(s.since.is_some());
        }
    }
    assert_eq!(native_session(&proc_root, Source::Dolphin), None);
    fs::write(proc_root.join("124/fdinfo/3"), "flags:\t0100000\n").unwrap();
    assert_eq!(
        native_session(&proc_root, Source::Dolphin).unwrap().pid,
        123
    );
    fs::write(proc_root.join("123/stat"), "123 (dolphin-emu) Z").unwrap();
    assert_eq!(native_session(&proc_root, Source::Dolphin), None);
}

#[test]
#[ignore = "reads the live Dolphin log and exports a frame only when explicitly requested"]
fn live_dolphin_log() {
    let dir =
        PathBuf::from(std::env::var_os("G13MAP_DOLPHIN_DUMP").expect("set G13MAP_DOLPHIN_DUMP"));
    let session =
        native_session(Path::new("/proc"), Source::Dolphin).expect("one native Dolphin logger");
    let mut m = Model::default();
    let mut tail = Tail::default();
    for _ in 0..=DOLPHIN_HISTORY / BATCH {
        tail.poll(&session.path, &mut m, Source::Dolphin, session.since)
            .unwrap();
        if !tail.bootstrap {
            break;
        }
    }
    assert!(m.game.is_some(), "live session identity");
    assert!(m.earned.is_some(), "live baseline");
    println!(
        "Dolphin game={:?}, title={}, earned={:?}/{:?}, hardcore={:?}, session=+{}",
        m.game, m.title, m.earned, m.total, m.hardcore, m.gained
    );
    fs::create_dir_all(&dir).unwrap();
    let bm = Display::default().frame_for(&mut m, Instant::now(), Source::Dolphin);
    fs::write(dir.join("live.lpbm"), &bm.0).unwrap();
    let mut bytes = b"P4\n160 43\n".to_vec();
    for y in 0..43 {
        for col in 0..20 {
            let mut b = 0;
            for bit in 0..8 {
                if !bm.get(col * 8 + bit, y) {
                    b |= 0x80 >> bit;
                }
            }
            bytes.push(b);
        }
    }
    fs::write(dir.join("live.pbm"), bytes).unwrap();
}
#[test]
#[ignore = "exports LCD review frames when G13MAP_RA_DUMP is set"]
fn dump_frames() {
    let Some(dir) = std::env::var_os("G13MAP_RA_DUMP") else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).unwrap();
    let now = Instant::now();
    let mut model = Model {
        game: Some(773),
        title: "Mega Man Battle Network 2".into(),
        earned: Some(4),
        total: Some(80),
        hardcore: Some(false),
        ..Model::default()
    };
    let mut display = Display::default();
    let mut pictures = vec![("overview".to_string(), display.frame(&mut model, now))];
    model.queue.push_back("A Chip Off The Old Block".into());
    pictures.push(("trophy".into(), display.frame(&mut model, now)));
    model.queue.push_back(
        "A Very Long Achievement Title That Must Marquee With One Second At Both Ends".into(),
    );
    for (name, millis) in [
        ("start", 8000),
        ("start-hold", 8900),
        ("middle", 13000),
        ("end", 17000),
        ("end-hold", 17900),
        ("returned", 18000),
    ] {
        pictures.push((
            name.into(),
            display.frame(&mut model, now + Duration::from_millis(millis)),
        ));
    }
    for (name, bm) in pictures {
        fs::write(dir.join(format!("{name}.lpbm")), &bm.0).unwrap();
        let mut bytes = b"P4\n160 43\n".to_vec();
        for y in 0..43 {
            for column in 0..20 {
                let mut byte = 0;
                for bit in 0..8 {
                    if !bm.get(column * 8 + bit, y) {
                        byte |= 0x80 >> bit;
                    }
                }
                bytes.push(byte);
            }
        }
        fs::write(dir.join(format!("{name}.pbm")), bytes).unwrap();
    }
}
#[test]
fn progress_confirmations_and_repeat_events() {
    let mut m = Model::default();
    m.ingest(Event::Identity(773, "Battle Network 2".into()), false);
    m.ingest(Event::Summary(4, 80), false);
    m.ingest(Event::Unlock(8, "Title".into()), true);
    m.ingest(Event::Unlock(8, "Title".into()), true);
    assert_eq!(m.queue.len(), 2);
    assert_eq!(m.earned, Some(4));
    m.ingest(Event::Awarded(8), true);
    m.ingest(Event::Awarded(8), true);
    assert_eq!(m.earned, Some(5));
    assert_eq!(m.gained, 1);
    m.ingest(Event::Identity(9, "Other game".into()), true);
    assert!(m.queue.is_empty());
    assert_eq!(m.earned, None);
    assert_eq!(
        event("[RCHEEVOS] You have 90 of 80 achievements unlocked"),
        None
    );
    assert_eq!(event("[RCHEEVOS] Achievement 8: already unlocked"), None);
}
#[test]
fn mode_tracks_native_changes_without_reusing_other_mode_count() {
    let mut m = Model::default();
    for line in [
        "[RCHEEVOS] Identified game: 773 \"GAME\" (hash)",
        "[RCHEEVOS] Game 773 loaded, hardcore disabled",
        "[RCHEEVOS] You have 4 of 80 achievements unlocked",
        "[RCHEEVOS] Hardcore enabled, waiting for reset",
    ] {
        m.ingest(event(line).unwrap(), true);
    }
    assert_eq!(m.hardcore, Some(true));
    assert_eq!((m.earned, m.total), (None, Some(80)));
    m.ingest(Event::Summary(2, 80), true);
    m.ingest(event("[RCHEEVOS] Hardcore enabled").unwrap(), true);
    assert_eq!(m.earned, Some(2));
    m.ingest(event("[RCHEEVOS] Hardcore disabled").unwrap(), true);
    assert_eq!(m.hardcore, Some(false));
    assert_eq!(m.earned, None);
    assert_eq!(event("[RCHEEVOS] Game 773 loaded, hardcore junk"), None);
    assert_eq!(event("[RCHEEVOS] Hardcore enabled-ish"), None);
    assert_eq!(
        event("[RCHEEVOS] Game 773 loaded, hardcore enabled, spectating"),
        Some(Event::Loaded(773, true))
    );
}
#[test]
fn native_session_requires_a_live_emulator_with_an_open_writable_log() {
    use std::os::unix::fs::symlink;
    let _sandbox = crate::test_support::Sandbox::new("ra-native");
    let proc_root = crate::state_dir().join("proc");
    let process = proc_root.join("123");
    fs::create_dir_all(process.join("fd")).unwrap();
    fs::create_dir_all(process.join("fdinfo")).unwrap();
    fs::write(process.join("comm"), "retroarch\n").unwrap();
    let mut fields = vec!["0"; 20];
    fields[0] = "S";
    fields[19] = "100";
    fs::write(
        process.join("stat"),
        format!("123 (retroarch) {}", fields.join(" ")),
    )
    .unwrap();
    let log = crate::state_dir().join("retroarch__2026_10_09__17_00_00.log");
    fs::write(&log, "[RCHEEVOS] Identified game: 1 \"GAME\" (hash)\n").unwrap();
    assert_eq!(native_session(&proc_root, Source::RetroArch), None); // An old file isn't a session.
    symlink(&log, process.join("fd/3")).unwrap();
    fs::write(process.join("fdinfo/3"), "flags:\t0100000\n").unwrap();
    assert_eq!(native_session(&proc_root, Source::RetroArch), None); // A read-only file isn't the logger.
    fs::write(process.join("fdinfo/3"), "flags:\t0102001\n").unwrap();
    let old = native_session(&proc_root, Source::RetroArch).unwrap();
    let mut model = Model::default();
    Tail::default()
        .poll(&old.path, &mut model, Source::RetroArch, None)
        .unwrap();
    assert_eq!(model.game, Some(1));
    fields[19] = "200";
    fs::write(
        process.join("stat"),
        format!("123 (retroarch) {}", fields.join(" ")),
    )
    .unwrap();
    assert_ne!(native_session(&proc_root, Source::RetroArch).unwrap(), old); // PID reuse resets the baseline.
    fields[0] = "Z";
    fs::write(
        process.join("stat"),
        format!("123 (retroarch) {}", fields.join(" ")),
    )
    .unwrap();
    assert_eq!(native_session(&proc_root, Source::RetroArch), None);
}
#[test]
fn queue_timers_and_marquee_end_holds() {
    let now = Instant::now();
    let mut d = Display::default();
    let mut m = Model {
        game: Some(1),
        ..Model::default()
    };
    m.queue.extend(["Short title".into(),"A very long achievement title with enough words to require scrolling across both lines of the display".into()]);
    d.frame(&mut m, now);
    assert_eq!(d.card.as_ref().unwrap().title, "Short title");
    d.frame(&mut m, now + Duration::from_millis(7999));
    assert_eq!(m.queue.len(), 1);
    d.frame(&mut m, now + Duration::from_secs(8));
    assert_eq!(m.queue.len(), 0);
    assert_eq!(d.card.as_ref().unwrap().duration, Duration::from_secs(10));
    assert_eq!(scroll_offset(400, 0, 8000), 0);
    assert_eq!(scroll_offset(400, 1000, 8000), 0);
    assert_eq!(scroll_offset(400, 9000, 8000), 240);
    assert_eq!(scroll_offset(400, 9999, 8000), 240);
    d.frame(&mut m, now + Duration::from_secs(18));
    assert!(d.card.is_none());
}
#[test]
fn tail_keeps_partial_lines_and_skips_old_notifications() {
    let _sandbox = crate::test_support::Sandbox::new("ra-tail");
    let path = crate::state_dir().join("ra.log");
    fs::create_dir_all(crate::state_dir()).unwrap();
    fs::write(&path,"[RCHEEVOS] Identified game: 1 \"GAME\" (hash)\n[RCHEEVOS] You have 2 of 10 achievements unlocked\n[RCHEEVOS] Awarding achievement 5: Old\n").unwrap();
    let mut tail = Tail::default();
    let mut m = Model::default();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert!(m.queue.is_empty());
    assert_eq!(m.earned, Some(2));
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    write!(file, "[RCHEEVOS] Awarding achievement 6: New").unwrap();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert!(m.queue.is_empty());
    writeln!(file, " title").unwrap();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert_eq!(m.queue.pop_front().as_deref(), Some("New title"));
    writeln!(file, "[RCHEEVOS] Achievement 6 awarded, new score: 4406").unwrap();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert_eq!((m.earned, m.gained), (Some(3), 1));
    // Focus changes restart the reader. Reopening reconstructs progress,
    // while the notification from this log stays suppressed.
    let mut restarted = Model::default();
    Tail::default()
        .poll(&path, &mut restarted, Source::RetroArch, None)
        .unwrap();
    assert_eq!((restarted.earned, restarted.gained), (Some(3), 1));
    assert!(restarted.queue.is_empty());
    writeln!(file, "[RCHEEVOS] Achievement 6 awarded, new score: 4406").unwrap();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert_eq!(m.gained, 1);
    fs::write(&path, "[RCHEEVOS] Identified game: 2 \"OTHER\" (hash)\n").unwrap();
    tail.poll(&path, &mut m, Source::RetroArch, None).unwrap();
    assert_eq!(m.game, Some(2));
    assert_eq!(m.earned, None);
    assert_eq!(m.gained, 0);
}
