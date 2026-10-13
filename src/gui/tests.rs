// SPDX-License-Identifier: GPL-3.0-or-later
use super::binding::evdev_name;
use super::{App, Mode};
use crate::gamepad::{Click, Mapping, Stick};
use crate::keys::KEYS;
use crate::profile::{Display, Profile, StickMode};
use crate::{daemon, lcd};
use egui::Key;
use std::time::Duration;
#[test]
fn audio_binding_and_system_tool_save_revert_keep_other_bindings() {
    let _sandbox = crate::test_support::Sandbox::new("audio-binding");
    std::env::set_var("G13MAP_ANALOG", "0");
    let pipe = crate::test_support::PanelPipe::new();
    let saved = Profile::parse("# pressure on\n# stick keys\nbind G1 KEY_A\nbind G2 KEY_B\n").0;
    crate::save("audio", &saved).unwrap();
    crate::set_active("audio").unwrap();
    let mut app = App::new("audio".into()).unwrap();
    pipe.read();
    app.select(Some("G1"));
    app.set_action(Some(crate::audio::Action::MicToggle.binding()));
    assert!(!app.status_err, "{}", app.status);
    pipe.read();
    app.select(Some("G1"));
    assert!(app.mode == Mode::Audio);
    app.profile.display = Display::Audio;
    app.profile.audio_mic = Some("usb.microphone".into());
    app.do_save();
    let loaded = crate::load("audio").unwrap().0;
    assert_eq!(loaded.display, Display::Audio);
    assert_eq!(loaded.audio_mic.as_deref(), Some("usb.microphone"));
    assert_eq!(loaded.binds["G2"], "KEY_B");
    assert!(!app.analog);
    app.profile.display = Display::Pressure;
    app.profile.audio_mic = None;
    app.do_revert();
    assert_eq!(app.profile, loaded);
    pipe.read();
}
#[test]
fn controller_binding_enables_adapter_and_revert_restores_keyboard_profile() {
    let _sandbox = crate::test_support::Sandbox::new("controller-binding");
    let pipe = crate::test_support::PanelPipe::new();
    let saved = Profile::parse("# stick keys\nbind G1 KEY_A\nbind G2 KEY_B\n").0;
    crate::save("one", &saved).unwrap();
    crate::set_active("one").unwrap();
    let mut app = App::new("one".into()).unwrap();
    pipe.read();
    app.select(Some("G1"));
    app.set_action(Some("PAD_LT".into()));
    assert!(!app.status_err, "{}", app.status);
    assert!(app.analog && app.dirty());
    let commands = String::from_utf8(pipe.read()).unwrap();
    assert!(commands.contains("bind G1 PAD_LT\n"));
    assert!(commands.contains("bind G2 KEY_B\n"));
    app.select(Some("G1"));
    assert!(app.mode == Mode::Controller);
    assert_eq!(crate::load("one").unwrap().0, saved);
    app.do_revert();
    assert_eq!(app.profile, saved);
    assert!(!app.analog && !app.status_err);
    pipe.read();
    app.set_action(Some("PAD_LT".into()));
    pipe.read();
    app.do_save();
    let controller = crate::load("one").unwrap().0;
    assert_eq!(controller.stick, Some(StickMode::Analog));
    assert_eq!(controller.binds["G1"], "PAD_LT");
    app.select(Some("G1"));
    app.set_action(Some("KEY_C".into()));
    pipe.read();
    app.do_revert();
    assert_eq!(app.profile, controller);
    assert!(!app.status_err, "{}", app.status);
}
#[test]
fn revert_unbinds_unsaved_live_keys_without_saving_transition_commands() {
    let _sandbox = crate::test_support::Sandbox::new("revert-unsaved-key");
    let pipe = crate::test_support::PanelPipe::new();
    let saved = Profile::parse("# stick keys\nbind G1 KEY_A\n").0;
    crate::save("one", &saved).unwrap();
    crate::set_active("one").unwrap();
    let mut app = App::new("one".into()).unwrap();
    pipe.read(); // initial LCD
    app.select(Some("G2"));
    app.set_action(Some("KEY_B".into()));
    assert_eq!(String::from_utf8(pipe.read()).unwrap(), "bind G2 KEY_B\n");
    assert!(app.dirty());
    app.do_revert();
    assert!(!app.status_err, "{}", app.status);
    assert!(String::from_utf8(pipe.read())
        .unwrap()
        .contains("bind G2 KEY_RESERVED\n"));
    assert_eq!(app.profile, saved);
    assert_eq!(crate::load("one").unwrap().0, saved);
    assert_eq!(crate::active_name(), "one");
    assert!(!app.dirty());
}
#[test]
fn gamepad_mapping_saves_reverts_resets_and_switches_per_profile() {
    let _sandbox = crate::test_support::Sandbox::new("profile-gamepad");
    let pipe = crate::test_support::PanelPipe::new();
    let profile = Profile::parse("# stick analog\nbind TOP KEY_C\n").0;
    crate::save("one", &profile).unwrap();
    crate::save("two", &profile).unwrap();
    crate::set_active("one").unwrap();
    let mut app = App::new("one".into()).unwrap();
    pipe.read();
    let custom = Mapping {
        stick: Stick::Right,
        click: Click::R3,
        swap: true,
        invert_x: true,
        invert_y: false,
    };
    app.set_gamepad(Some(custom));
    assert!(!app.status_err, "{}", app.status);
    pipe.read();
    assert!(app.dirty());
    assert_eq!(
        Mapping::read(&std::path::PathBuf::from(
            std::env::var_os("G13MAP_ANALOG_MAP").unwrap()
        ))
        .unwrap(),
        custom
    );
    app.do_save();
    assert_eq!(crate::load("one").unwrap().0.gamepad, Some(custom));
    app.set_gamepad(None);
    pipe.read();
    assert!(app.dirty());
    app.do_revert();
    pipe.read();
    assert_eq!(app.profile.gamepad, Some(custom));
    app.switch("two".into());
    pipe.read();
    pipe.read();
    assert_eq!(app.profile.gamepad, None);
    assert_eq!(
        Mapping::read(&std::path::PathBuf::from(
            std::env::var_os("G13MAP_ANALOG_MAP").unwrap()
        ))
        .unwrap(),
        Mapping::default()
    );
    drop(pipe);
    app.set_gamepad(Some(custom));
    assert!(app.status_err);
    assert_eq!(app.profile.gamepad, None);
}
#[test]
fn stick_preference_saves_reverts_and_switches_with_top_ownership() {
    let _sandbox = crate::test_support::Sandbox::new("profile-stick");
    let pipe = crate::test_support::PanelPipe::new();
    let keys = Profile::parse("# stick keys\nbind TOP KEY_C\n").0;
    let analog = Profile::parse("# stick analog\nbind TOP KEY_D\n").0;
    crate::save("keys", &keys).unwrap();
    crate::save("analog", &analog).unwrap();
    crate::set_active("analog").unwrap();
    let mut app = App::new("analog".into()).unwrap();
    assert_eq!(pipe.read(), lcd::Bitmap::logo().0);
    app.switch("keys".into());
    let commands = String::from_utf8(pipe.read()).unwrap();
    assert!(commands.contains("bind TOP KEY_C\n"));
    assert_eq!(pipe.read(), lcd::Bitmap::logo().0);
    assert!(!app.analog && !daemon::analog_active());
    app.set_stick(Some(StickMode::Analog));
    assert!(!String::from_utf8(pipe.read()).unwrap().contains("bind TOP"));
    assert!(app.dirty() && app.analog);
    app.do_revert();
    assert!(String::from_utf8(pipe.read())
        .unwrap()
        .contains("bind TOP KEY_C"));
    assert!(!app.dirty() && !app.analog);
    app.set_stick(Some(StickMode::Analog));
    let _commands = pipe.read();
    app.do_save();
    assert_eq!(
        crate::load("keys").unwrap().0.stick,
        Some(StickMode::Analog)
    );
    // A missing daemon must not switch the active file or the editor's selection.
    drop(pipe);
    // Add an ordinary key to force a send even with no daemon baseline.
    let mut target = keys;
    target.binds.insert("G1".into(), "KEY_A".into());
    crate::save("failed", &target).unwrap();
    let before = app.name.clone();
    app.switch("failed".into());
    assert!(app.status_err);
    assert_eq!(app.name, before);
    assert_eq!(crate::active_name(), before);
}
#[test]
fn external_profile_switch_replaces_editor_animation() {
    let _sandbox = crate::test_support::Sandbox::new("editor-switch");
    let pipe = crate::test_support::PanelPipe::new();
    crate::test_support::picture("old", &[11, 12]);
    crate::test_support::picture("new", &[21, 22]);
    crate::set_active("old").unwrap();
    let mut app = App::new("old".into()).unwrap();
    pipe.expect_frames(&[11, 12], 3);
    crate::set_active("new").unwrap();
    app.follow_active();
    assert_eq!(app.name, "new");
    // Discard frames queued before the replacement's first frame.
    loop {
        let frame = pipe.read();
        if frame == vec![21; lcd::BYTES] {
            break;
        }
        assert!([11, 12].contains(&frame[0]), "unexpected frame");
        assert!(
            app.active_checked.elapsed() < Duration::from_secs(1),
            "old animation kept playing"
        );
    }
    pipe.expect_frames(&[21, 22], 4);
    drop(app);
}

#[test]
fn editor_takes_over_playback_on_open() {
    let _sandbox = crate::test_support::Sandbox::new("editor-open");
    let pipe = crate::test_support::PanelPipe::new();
    crate::test_support::picture("moving", &[31, 32]);
    let app = App::new("moving".into()).unwrap();
    assert!(lcd::editor_open());
    assert!(
        app.player.is_some(),
        "editor paused the watcher without taking over playback"
    );
    pipe.expect_frames(&[31, 32], 4);
    drop(app);
}

#[test]
fn watcher_yields_first_frame_until_editor_closes() {
    let _sandbox = crate::test_support::Sandbox::new("editor-yield");
    let pipe = crate::test_support::PanelPipe::new();
    crate::test_support::picture("moving", &[41, 42]);
    let lock = lcd::editor_lock().unwrap();
    let player = lcd::show(Some("moving"), true).unwrap();
    pipe.expect_quiet();
    drop(lock);
    pipe.expect_frames(&[41, 42], 4);
    drop(player);
}

#[test]
fn manual_profile_switches_replace_animation_with_still_and_logo() {
    let _sandbox = crate::test_support::Sandbox::new("editor-manual");
    let pipe = crate::test_support::PanelPipe::new();
    crate::test_support::picture("moving", &[51, 52]);
    crate::test_support::picture("still", &[61]);
    crate::save("logo", &Profile::default()).unwrap();
    let mut app = App::new("moving".into()).unwrap();
    pipe.expect_frames(&[51, 52], 3);
    app.switch("still".into());
    assert!(!app.status_err, "{}", app.status);
    loop {
        let frame = pipe.read();
        if frame == vec![61; lcd::BYTES] {
            break;
        }
        // Profile binds are text reads; any intervening animation is the old one.
        if frame.len() == lcd::BYTES {
            assert!([51, 52].contains(&frame[0]));
        }
    }
    pipe.expect_quiet();
    app.switch("logo".into());
    assert!(!app.status_err, "{}", app.status);
    let logo = lcd::Bitmap::logo();
    loop {
        let frame = pipe.read();
        if frame == logo.0 {
            break;
        }
        assert_ne!(frame.len(), lcd::BYTES, "old animation restarted");
    }
    pipe.expect_quiet();
    drop(app);
}

#[test]
fn every_captured_name_is_bindable() {
    for k in [
        Key::A,
        Key::F24,
        Key::ArrowUp,
        Key::Backtick,
        Key::IntlBackslash,
        Key::SuperLeft,
        Key::Num0,
    ] {
        let n = evdev_name(k).unwrap();
        assert!(KEYS.iter().any(|&(_, x)| x == n), "{n}");
    }
    assert!(crate::profile::CONTROLS.contains(&"G22"));
}
