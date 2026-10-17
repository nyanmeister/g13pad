// SPDX-License-Identifier: GPL-3.0-or-later
//! Temporary LCD playback, with a separate process lock and no saved-picture changes.
use crate::{daemon, lcd, overlay};
use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime},
};

const HELP: &str = "usage: g13map demo FILE [SECONDS]
       g13map demo scene NAME [SECONDS]   show a built-in animation (list: g13map demo scenes)
       g13map demo scenes                 list the built-in animations
       g13map demo agent [SECONDS]        play the coding-agent screen's lifecycle
Show an image or animation on the LCD for SECONDS (default: 10).
Positive fractional seconds are accepted; Ctrl+C restores the current display.
GIF/APNG/WebP animations repeat with their frame timings. Native sprite sheets
use 160x43 cells, left-to-right then top-to-bottom, at 200 ms per frame.
Ordinary images, driver .lpbm frames and kept .anim files also work.
White image pixels light the LCD. Profiles and kept pictures are preserved.
The watcher restores the active display after a force quit.
g13map demo --clear restores it manually when no demo or editor is running.";
const MAX_NATIVE: u64 = 16 << 20;
const POLL: Duration = Duration::from_millis(25);
static STOP: AtomicBool = AtomicBool::new(false);

pub(crate) fn command(args: &[&str]) -> Result<String, String> {
    if args.is_empty() || matches!(args, ["--help" | "-h"]) {
        return Ok(HELP.into());
    }
    if args == ["--clear"] {
        clear()?;
        return Ok("demo cleared; the active profile's display can resume".into());
    }
    match args {
        ["scenes"] => return scenes(),
        ["scene", rest @ ..] => return scene(rest),
        ["agent", rest @ ..] => return agent_demo(rest),
        _ => {}
    }
    let (path, duration) = request(args)?;
    // The packaged CLI stays lean; its sibling decodes images without opening a GUI.
    #[cfg(not(feature = "editor"))]
    if !native(path) {
        use std::os::unix::process::CommandExt;
        let error = crate::cli::editor_command()?
            .arg("--demo")
            .args(args)
            .exec();
        return Err(format!(
            "image demos need the g13map-editor component: {error}"
        ));
    }
    let _signals = Signals::take()?;
    let animation = load(path)?;
    if STOP.load(Ordering::Relaxed) {
        return Ok("demo cancelled before display".into());
    }
    let mut owner = Owner::take()?;
    eprintln!(
        "demo: {} ({} frames), for {} seconds",
        path.display(),
        animation.frames.len(),
        duration.as_secs_f64()
    );
    let played = play(&animation, duration, &STOP);
    let restored = owner.restore();
    played.and(restored)?;
    Ok("demo finished; normal display can resume".into())
}

fn request<'a>(args: &[&'a str]) -> Result<(&'a Path, Duration), String> {
    let (path, seconds) = match args {
        [path] => (*path, "10"),
        [path, seconds] => (*path, *seconds),
        _ => return Err(HELP.into()),
    };
    Ok((Path::new(path), parse_seconds(seconds)?))
}

/// SECONDS as a positive, finite duration within the timer's range.
fn parse_seconds(seconds: &str) -> Result<Duration, String> {
    let seconds: f64 = seconds
        .parse()
        .map_err(|_| "SECONDS must be a positive finite number".to_string())?;
    Duration::try_from_secs_f64(seconds)
        .ok()
        .filter(|d| !d.is_zero() && Instant::now().checked_add(*d).is_some())
        .ok_or_else(|| "SECONDS must be positive, finite and within the timer's range".to_string())
}

/// The built-in animations (`art`): list them, and show one on the LCD like a file demo.
#[cfg(feature = "art")]
fn scenes() -> Result<String, String> {
    let list = crate::art::SCENES
        .iter()
        .map(|s| format!("  {:<14} {}", s.name, s.about))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "built-in scenes, shown with `g13map demo scene NAME`:\n{list}"
    ))
}

#[cfg(feature = "art")]
fn scene(args: &[&str]) -> Result<String, String> {
    let (name, seconds) = match args {
        [name] => (*name, "10"),
        [name, seconds] => (*name, *seconds),
        _ => return Err("usage: g13map demo scene NAME [SECONDS]".into()),
    };
    let duration = parse_seconds(seconds)?;
    // A scene this binary lacks may be kept under its name by a newer `g13map-anim keep`.
    let animation = match crate::art::Scene::find(name) {
        Some(scene) => scene.animation(),
        None => lcd::load_anim(name).ok_or_else(|| {
            format!("no scene '{name}' built in or kept; see `g13map demo scenes`")
        })?,
    };
    let _signals = Signals::take()?;
    if STOP.load(Ordering::Relaxed) {
        return Ok("demo cancelled before display".into());
    }
    let mut owner = Owner::take()?;
    eprintln!(
        "demo: scene {name} ({} frames), for {} seconds",
        animation.frames.len(),
        duration.as_secs_f64()
    );
    let played = play(&animation, duration, &STOP);
    let restored = owner.restore();
    played.and(restored)?;
    Ok("demo finished; normal display can resume".into())
}

// The packaged CLI carries no `art`; it re-runs itself as the editor sibling, which does,
// exactly as the image-demo path hands off a format it cannot decode.
#[cfg(not(feature = "art"))]
fn scenes() -> Result<String, String> {
    delegate_scene(&["scenes"])
}

#[cfg(not(feature = "art"))]
fn scene(args: &[&str]) -> Result<String, String> {
    let mut forward = vec!["scene"];
    forward.extend_from_slice(args);
    delegate_scene(&forward)
}

#[cfg(not(feature = "art"))]
fn delegate_scene(args: &[&str]) -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let error = crate::cli::editor_command()?
        .arg("--demo")
        .args(args)
        .exec();
    Err(format!(
        "built-in scenes need the g13map-editor component: {error}"
    ))
}

/// The canned lifecycle for `g13map demo agent`, as a function of time into a 20 s loop:
/// a turn editing then searching (with a tool-failure and an edit-denial flash), a
/// permission block, a finished turn, then idle. `wall` is only used to time the flashes.
fn demo_state(
    tt: f64,
    started: SystemTime,
    wall: SystemTime,
    prev: Duration,
    last: Duration,
) -> (crate::agent::State, Duration) {
    use crate::agent::{Activity, State, Trailer};
    let base = Trailer {
        agents: 2,
        last: Some(prev),
        ..Default::default()
    };
    if tt < 8.0 {
        let activity = if tt < 4.0 { "EDITING" } else { "WEB_SEARCH" };
        let mut trailer = base;
        if (2.5..2.9).contains(&tt) {
            trailer.fail_at = Some(wall); // orange flash
        }
        if (6.0..6.4).contains(&tt) {
            trailer.deny_at = Some(wall); // red flash
        }
        (
            State::Turn {
                activity: Activity::parse(activity),
                started,
                trailer,
            },
            Duration::from_secs_f64(tt),
        )
    } else if tt < 12.0 {
        (
            State::Ask {
                attention: crate::agent::Attention::Permission,
                started: None,
                trailer: base,
            },
            Duration::from_secs_f64(tt - 8.0),
        )
    } else if tt < 15.0 {
        (
            State::Ready {
                trailer: Trailer {
                    agents: 2,
                    last: Some(last),
                    ..Default::default()
                },
            },
            Duration::from_secs_f64(tt - 12.0),
        )
    } else {
        (
            State::Idle {
                trailer: Trailer {
                    agents: 2,
                    last: Some(last),
                    ..Default::default()
                },
            },
            Duration::from_secs_f64(tt - 15.0),
        )
    }
}

/// Plays the agent screen's lifecycle on the glass with its backlight, for SECONDS, then
/// restores the active profile's picture and colour. A showcase before the real feeder
/// exists; it takes the same demo lock as a file demo, so the watcher stays clear.
fn agent_demo(args: &[&str]) -> Result<String, String> {
    use crate::agent::render;
    let seconds = match args {
        [] => "24",
        [s] => *s,
        _ => return Err("usage: g13map demo agent [SECONDS]".into()),
    };
    let duration = parse_seconds(seconds)?;
    let _signals = Signals::take()?;
    if STOP.load(Ordering::Relaxed) {
        return Ok("demo cancelled before display".into());
    }
    let mut owner = Owner::take()?;
    // Put the active profile's backlight back at the end (its own, else the daemon default).
    let restore_rgb = crate::load(&crate::active_name())
        .ok()
        .and_then(|(p, _)| p.rgb)
        .unwrap_or([31, 0, 127]);
    // The working backlight the mockup used; the alert phases override it.
    const WORKING: [u8; 3] = [0, 200, 255];
    let started = SystemTime::now() - Duration::from_secs(83);
    let (prev, last) = (
        Duration::from_millis(178_900),
        Duration::from_millis(174_890),
    );
    eprintln!(
        "demo: agent lifecycle, for {} seconds",
        duration.as_secs_f64()
    );
    let start = Instant::now();
    let mut last_rgb: Option<[u8; 3]> = None;
    let mut result = Ok(());
    while !STOP.load(Ordering::Relaxed) {
        let elapsed = start.elapsed();
        if elapsed >= duration {
            break;
        }
        if lcd::editor_open() || overlay::active()? {
            thread::sleep(POLL);
            continue;
        }
        let wall = SystemTime::now();
        let (state, age) = demo_state(elapsed.as_secs_f64() % 20.0, started, wall, prev, last);
        let frame = render::frame(&state, age, wall);
        let rgb = render::colour(&state, age, wall, WORKING);
        if last_rgb != Some(rgb) {
            if let Err(e) = daemon::send(&format!("rgb {} {} {}\n", rgb[0], rgb[1], rgb[2])) {
                result = Err(e);
                break;
            }
            last_rgb = Some(rgb);
        }
        if let Err(e) = send(&frame) {
            result = Err(e);
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let [r, g, b] = restore_rgb;
    let _ = daemon::send(&format!("rgb {r} {g} {b}\n"));
    let restored = owner.restore();
    result.and(restored)?;
    Ok("demo finished; normal display can resume".into())
}

fn native(path: &Path) -> bool {
    path.extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("lpbm") || s.eq_ignore_ascii_case("anim"))
}

fn load(path: &Path) -> Result<lcd::Animation, String> {
    let file = File::options()
        .read(true)
        .custom_flags(0o4000 | 0o2000000)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("demo input must be a regular file".into());
    }
    if native(path) {
        let mut bytes = Vec::new();
        file.take(MAX_NATIVE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_NATIVE {
            return Err("native demo input exceeds 16 MiB".into());
        }
        return if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("lpbm"))
        {
            lcd::Bitmap::from_lpbm(&bytes).map(lcd::Animation::still)
        } else {
            lcd::Animation::from_bytes(&bytes)
        };
    }
    #[cfg(feature = "editor")]
    {
        let source = lcd::Source::open(path)?;
        let options = lcd::Options {
            invert: true,
            background: 0,
            ..Default::default()
        };
        let columns = source.w / lcd::W as u32;
        let rows = source.h / lcd::H as u32;
        if !source.animated()
            && source.w.is_multiple_of(lcd::W as u32)
            && source.h.is_multiple_of(lcd::H as u32)
            && u64::from(columns) * u64::from(rows) > 1
        {
            if u64::from(columns) * u64::from(rows) > 4096 {
                return Err("sprite sheet exceeds 4096 frames".into());
            }
            let mut frames = Vec::with_capacity((columns * rows) as usize);
            for row in 0..rows {
                for column in 0..columns {
                    let options = lcd::Options {
                        crop: Some(lcd::Crop {
                            x: (column * lcd::W as u32) as f32,
                            y: (row * lcd::H as u32) as f32,
                            w: lcd::W as f32,
                            h: lcd::H as f32,
                        }),
                        ..options
                    };
                    frames.push((Duration::from_millis(200), source.render(&options)));
                }
            }
            return Ok(lcd::Animation { frames });
        }
        Ok(source.render_all(&options))
    }
    #[cfg(not(feature = "editor"))]
    Err("image demos need the g13map-editor component".into())
}

fn lock_file(create: bool) -> Result<File, std::io::Error> {
    let path = daemon::runtime_file("demo").map_err(std::io::Error::other)?;
    fs::OpenOptions::new()
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(0o400000 | 0o2000000)
        .open(path)
}

/// The OS releases this lock even on SIGKILL; there is no stale PID/expiry to clear.
pub(crate) fn active() -> bool {
    match lock_file(false) {
        Ok(file) => match file.try_lock() {
            Ok(()) => false,
            Err(_) => true,
        },
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

struct Owner(Option<File>);
impl Owner {
    fn take() -> Result<Self, String> {
        daemon::lcd_turn(|_| {
            if lcd::editor_open() {
                return Err("an editor owns the LCD; close it before starting a demo".into());
            }
            let file = lock_file(true).map_err(|e| e.to_string())?;
            file.try_lock()
                .map_err(|e| format!("another demo owns the LCD: {e}"))?;
            if overlay::remembered_frame().is_none() {
                let normal = saved_frame();
                overlay::remember(&normal)?;
            }
            // Record recovery before the first demo frame. A very short process can
            // start and die between two watcher polls, so lock transitions alone fail.
            fs::write(daemon::runtime_file("demo-pending")?, []).map_err(|e| e.to_string())?;
            Ok(Owner(Some(file)))
        })
    }

    fn restore(&mut self) -> Result<(), String> {
        if self.0.is_none() {
            return Ok(());
        }
        let result = daemon::lcd_turn(|file| {
            // Serialize the release and restoration with every ordinary LCD writer.
            self.0.take();
            restore_pending(file)
        });
        self.0.take(); // Always release ownership, even if the daemon disappeared.
        result
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn saved_frame() -> Vec<u8> {
    crate::load(&crate::active_name())
        .ok()
        .and_then(|(p, _)| p.lcd.and_then(|n| lcd::load(&n).ok()))
        .unwrap_or_else(lcd::Bitmap::logo)
        .0
}

fn clear() -> Result<(), String> {
    daemon::lcd_turn(|file| {
        if active() || lcd::editor_open() {
            return Err("a demo or editor still owns the LCD; refusing to interrupt it".into());
        }
        if overlay::active()? {
            return Ok(());
        }
        let frame = saved_frame();
        overlay::remember(&frame)?;
        overlay::clear_expired()?;
        daemon::write_atomic(file, &frame)?;
        clear_pending()
    })
}

fn clear_pending() -> Result<(), String> {
    match fs::remove_file(daemon::runtime_file("demo-pending")?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn restore_pending(file: &mut File) -> Result<(), String> {
    if lcd::editor_open() || overlay::active()? {
        return Ok(());
    }
    overlay::clear_expired()?;
    let frame = overlay::remembered_frame().unwrap_or_else(saved_frame);
    daemon::write_atomic(file, &frame)?;
    clear_pending()
}

/// The marker survives SIGKILL; the lock does not. Retry after reconnect or editor close.
pub(crate) fn recover() -> Result<(), String> {
    let pending = daemon::runtime_file("demo-pending")?;
    if !pending.exists() || active() || lcd::editor_open() {
        return Ok(());
    }
    daemon::lcd_turn(|file| {
        if !pending.exists() || active() {
            return Ok(());
        }
        restore_pending(file)
    })
}

/// Returns whether the demo frame was shown; normal frames remain in the restore cache.
fn send(frame: &lcd::Bitmap) -> Result<bool, String> {
    daemon::lcd_turn(|file| {
        if lcd::editor_open() || overlay::active()? {
            return Ok(false);
        }
        overlay::clear_expired()?;
        daemon::write_atomic(file, &frame.0)?;
        Ok(true)
    })
}

fn play(animation: &lcd::Animation, duration: Duration, stop: &AtomicBool) -> Result<(), String> {
    let start = Instant::now();
    let mut shown = None;
    while !stop.load(Ordering::Relaxed) {
        let elapsed = start.elapsed();
        if elapsed >= duration {
            break;
        }
        if lcd::editor_open() || overlay::active()? {
            shown = None;
            thread::sleep(POLL.min(duration.saturating_sub(elapsed)));
            continue;
        }
        let (index, next) = if animation.animated() {
            animation.due(elapsed)
        } else {
            (0, POLL)
        };
        if shown != Some(index) {
            shown = send(&animation.frames[index].1)?.then_some(index);
        }
        thread::sleep(next.min(POLL).min(duration.saturating_sub(start.elapsed())));
    }
    Ok(())
}

extern "C" fn on_stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
}
struct Signals([usize; 2]);
impl Signals {
    fn take() -> Result<Self, String> {
        STOP.store(false, Ordering::Relaxed);
        let handler = on_stop as extern "C" fn(i32) as usize;
        // SAFETY: the handler only updates a lock-free atomic flag; these are Linux signals.
        let interrupt = unsafe { signal(2, handler) };
        if interrupt == usize::MAX {
            return Err("cannot handle SIGINT".into());
        }
        let terminate = unsafe { signal(15, handler) };
        if terminate == usize::MAX {
            unsafe {
                signal(2, interrupt);
            }
            return Err("cannot handle SIGTERM".into());
        }
        Ok(Self([interrupt, terminate]))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        // SAFETY: restore the previous handlers returned by signal(), after display cleanup.
        unsafe {
            signal(2, self.0[0]);
            signal(15, self.0[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{PanelPipe, Sandbox};

    #[test]
    fn usage_and_bad_durations_do_not_touch_hardware() {
        let sandbox = Sandbox::new("demo-args");
        assert!(command(&[]).unwrap().contains("default: 10"));
        assert_eq!(request(&["any.gif"]).unwrap().1, Duration::from_secs(10));
        for seconds in ["0", "-1", "NaN", "inf", "1e300", "1e-300", "bad"] {
            assert!(command(&["missing.gif", seconds]).is_err(), "{seconds}");
        }
        assert!(!sandbox.dir.join("runtime").exists());
    }

    #[cfg(feature = "art")]
    #[test]
    fn scenes_listing_names_the_scenes_without_touching_hardware() {
        let sandbox = Sandbox::new("demo-scenes-list");
        let out = command(&["scenes"]).unwrap();
        assert!(out.contains("orrery") && out.contains("aquarium"), "{out}");
        assert!(!sandbox.dir.join("runtime").exists());
    }

    #[cfg(feature = "art")]
    #[test]
    fn an_unknown_scene_errors_before_any_display() {
        let sandbox = Sandbox::new("demo-scene-unknown");
        assert!(command(&["scene", "not-a-scene"]).is_err());
        assert!(!sandbox.dir.join("runtime").exists());
    }

    #[test]
    fn demo_preserves_latest_normal_frame_and_uses_a_separate_lock() {
        let _sandbox = Sandbox::new("demo-owner");
        let pipe = PanelPipe::new();
        daemon::send_lcd(&vec![3; lcd::BYTES]).unwrap();
        assert_eq!(pipe.read(), vec![3; lcd::BYTES]);
        let mut owner = Owner::take().unwrap();
        assert!(Owner::take().is_err());
        assert!(active());
        assert!(send(&lcd::Bitmap(vec![5; lcd::BYTES])).unwrap());
        assert_eq!(pipe.read(), vec![5; lcd::BYTES]);
        daemon::send_lcd(&vec![7; lcd::BYTES]).unwrap();
        pipe.expect_quiet();
        owner.restore().unwrap();
        assert_eq!(pipe.read(), vec![7; lcd::BYTES]);
        assert!(!active());
        let owner = Owner::take().unwrap();
        let editor = lcd::editor_lock().unwrap();
        daemon::send_lcd(&vec![9; lcd::BYTES]).unwrap();
        assert_eq!(pipe.read(), vec![9; lcd::BYTES]);
        assert!(!send(&lcd::Bitmap(vec![5; lcd::BYTES])).unwrap());
        drop(owner);
        pipe.expect_quiet();
        drop(editor);
    }

    #[test]
    fn native_formats_and_manual_clear_restore_the_current_profile() {
        let sandbox = Sandbox::new("demo-native");
        let pipe = PanelPipe::new();
        let path = sandbox.dir.join("test.lpbm");
        fs::write(&path, [4; lcd::BYTES]).unwrap();
        assert_eq!(load(&path).unwrap().first().0, vec![4; lcd::BYTES]);
        let animation = lcd::Animation {
            frames: vec![
                (Duration::from_millis(30), lcd::Bitmap(vec![4; lcd::BYTES])),
                (Duration::from_millis(40), lcd::Bitmap(vec![5; lcd::BYTES])),
            ],
        };
        let path = sandbox.dir.join("test.anim");
        fs::write(&path, animation.to_bytes()).unwrap();
        assert_eq!(load(&path).unwrap(), animation);
        let owner = Owner::take().unwrap();
        assert!(clear().is_err());
        drop(owner);
        pipe.read();
        fs::create_dir_all(lcd::dir()).unwrap();
        fs::write(lcd::path("current"), [12; lcd::BYTES]).unwrap();
        crate::save(
            "current",
            &crate::Profile {
                lcd: Some("current".into()),
                ..Default::default()
            },
        )
        .unwrap();
        crate::set_active("current").unwrap();
        clear().unwrap();
        assert_eq!(pipe.read(), vec![12; lcd::BYTES]);
    }

    #[test]
    fn force_quit_before_a_watcher_poll_still_recovers_after_editor_close() {
        let _sandbox = Sandbox::new("demo-kill");
        let pipe = PanelPipe::new();
        daemon::send_lcd(&vec![6; lcd::BYTES]).unwrap();
        pipe.read();
        let mut owner = Owner::take().unwrap();
        send(&lcd::Bitmap(vec![11; lcd::BYTES])).unwrap();
        assert_eq!(pipe.read(), vec![11; lcd::BYTES]);
        // Releasing only the file models the kernel's SIGKILL cleanup, with no Drop.
        owner.0.take();
        assert!(!active());
        let editor = lcd::editor_lock().unwrap();
        recover().unwrap();
        pipe.expect_quiet();
        assert!(daemon::runtime_file("demo-pending").unwrap().exists());
        drop(editor);
        recover().unwrap();
        assert_eq!(pipe.read(), vec![6; lcd::BYTES]);
        assert!(!daemon::runtime_file("demo-pending").unwrap().exists());
        recover().unwrap();
        pipe.expect_quiet();
    }

    #[test]
    #[cfg(feature = "editor")]
    fn native_sheet_order_polarity_and_gif_timing() {
        use image::{codecs::gif::GifEncoder, Delay, Frame, Rgba, RgbaImage};
        let sandbox = Sandbox::new("demo-images");
        let path = sandbox.dir.join("sheet.png");
        RgbaImage::from_fn(320, 86, |x, y| {
            let tile = x / 160 + 2 * (y / 43);
            let value = if x % 160 == tile { 255 } else { 0 };
            Rgba([value, value, value, 255])
        })
        .save(&path)
        .unwrap();
        let sheet = load(&path).unwrap();
        assert_eq!(sheet.frames.len(), 4);
        for (i, (delay, frame)) in sheet.frames.iter().enumerate() {
            assert_eq!(*delay, Duration::from_millis(200));
            assert!(frame.get(i, 0));
            assert!(!frame.get((i + 1) % 160, 0));
            assert_eq!(frame.lit(), 43);
        }
        let path = sandbox.dir.join("test.gif");
        let mut encoder = GifEncoder::new(File::create(&path).unwrap());
        for (value, ms) in [(0, 40), (255, 70)] {
            encoder
                .encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(160, 43, Rgba([value, value, value, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(ms, 1),
                ))
                .unwrap();
        }
        drop(encoder);
        let gif = load(&path).unwrap();
        assert_eq!(gif.frames[0].0, Duration::from_millis(40));
        assert_eq!(gif.frames[1].0, Duration::from_millis(70));
        assert_eq!(gif.frames[0].1.lit(), 0);
        assert_eq!(gif.frames[1].1.lit(), 160 * 43);
    }
}
