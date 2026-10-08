// SPDX-License-Identifier: GPL-3.0-or-later
//! Transient LCD errors across editor/watcher processes, serialized by the FIFO lock.
use crate::{daemon, lcd};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TTL: Duration = Duration::from_secs(5);
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn enabled() -> bool {
    read(&crate::config_dir().join("lcd-errors"), 16)
        .ok()
        .flatten()
        .is_none_or(|bytes| String::from_utf8_lossy(&bytes).trim() != "0")
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    fs::create_dir_all(crate::config_dir()).map_err(|e| e.to_string())?;
    store(
        &crate::config_dir().join("lcd-errors"),
        if enabled { b"1\n" } else { b"0\n" },
    )?;
    tick()
}

fn store(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!(
        "{}-{}.new",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(0o400000 | 0o2000000)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| fs::rename(&tmp, path))
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn read(path: &Path, max: usize) -> Result<Option<Vec<u8>>, String> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000 | 0o400000 | 0o2000000)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("LCD state must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("LCD state exceeds its size limit".into());
    }
    Ok(Some(bytes))
}

fn state() -> Result<Option<(u64, Vec<u8>)>, String> {
    let Some(bytes) = read(&daemon::runtime_file("overlay")?, 8 + lcd::BYTES)? else {
        return Ok(None);
    };
    if bytes.len() != 8 + lcd::BYTES {
        return Err("invalid LCD overlay state".into());
    }
    let expiry = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    Ok(Some((expiry, bytes[8..].to_vec())))
}

pub(crate) fn active() -> Result<bool, String> {
    Ok(enabled()
        && state()
            .ok()
            .flatten()
            .is_some_and(|(expiry, _)| expiry > now() && expiry <= now() + 10_000))
}

pub(crate) fn remember(frame: &[u8]) -> Result<(), String> {
    let path = daemon::runtime_file("frame")?;
    if read(&path, lcd::BYTES).ok().flatten().as_deref() != Some(frame) {
        store(&path, frame)?;
    }
    Ok(())
}

pub(crate) fn clear_expired() -> Result<(), String> {
    if !active()? {
        match fs::remove_file(daemon::runtime_file("overlay")?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(feature = "editor")]
pub(crate) fn error_frame(error: &str) -> Result<Vec<u8>, String> {
    let bounded = error
        .chars()
        .take(256)
        .filter(|c| !c.is_control() || c.is_whitespace())
        .take(92)
        .collect::<String>();
    let message = bounded.split_whitespace().collect::<Vec<_>>().join(" ");
    let options = crate::text_options::Options {
        size: 10.0,
        wrap: true,
        speed: 0,
        align: crate::text_options::Align::Left,
        ..Default::default()
    };
    let text = format!(
        "G13 error\n{}",
        if message.is_empty() {
            "Operation failed"
        } else {
            &message
        }
    );
    Ok(crate::marquee::render_with(&text, &options)?
        .first()
        .0
        .to_vec())
}

#[cfg(not(feature = "editor"))]
pub(crate) fn error_frame(error: &str) -> Result<Vec<u8>, String> {
    let bounded = error.chars().take(256).collect::<String>();
    let output = crate::cli::editor_command()?
        .args(["--error-frame", &bounded])
        .output()
        .map_err(|e| format!("LCD errors need the g13map-editor component: {e}"))?;
    if !output.status.success() || output.stdout.len() != crate::lcd::BYTES {
        return Err("LCD error renderer failed or returned an invalid frame".into());
    }
    Ok(output.stdout)
}

pub fn notify(error: &str) -> Result<(), String> {
    notify_for(error, TTL)
}

fn notify_for(error: &str, duration: Duration) -> Result<(), String> {
    if !enabled() || !daemon::up() {
        return Ok(());
    }
    let frame = error_frame(error)?;
    daemon::lcd_turn(|file| {
        if active()? && state()?.is_some_and(|(_, shown)| shown == frame) {
            return Ok(());
        }
        let mut marker = (now() + duration.as_millis().min(TTL.as_millis()) as u64)
            .to_le_bytes()
            .to_vec();
        marker.extend_from_slice(&frame);
        store(&daemon::runtime_file("overlay")?, &marker)?;
        if let Err(e) = daemon::write_atomic(file, &frame) {
            let _ = fs::remove_file(daemon::runtime_file("overlay")?);
            return Err(e);
        }
        Ok(())
    })
}

pub fn tick() -> Result<(), String> {
    if state().is_ok_and(|s| s.is_none()) || active()? {
        return Ok(());
    }
    daemon::lcd_turn(|file| {
        if state().is_ok_and(|s| s.is_none()) || active()? {
            return Ok(());
        }
        let frame = read(&daemon::runtime_file("frame")?, lcd::BYTES)
            .ok()
            .flatten()
            .filter(|bytes| bytes.len() == lcd::BYTES)
            .unwrap_or_else(|| lcd::LOGO.to_vec());
        clear_expired()?;
        daemon::write_atomic(file, &frame)
    })
}

#[cfg(all(test, feature = "editor"))]
mod tests {
    use super::*;
    #[test]
    fn cross_process_worker() {
        let Ok(task) = std::env::var("G13PAD_OVERLAY_TEST_TASK") else {
            return;
        };
        assert_eq!(std::env::var("G13MAP_UNIT").unwrap(), "0");
        match task.as_str() {
            "error" => notify_for("Cross-process failure", Duration::from_millis(400)).unwrap(),
            "duplicate" => notify("Cross-process failure").unwrap(),
            "frame" => daemon::send_lcd(&vec![62; lcd::BYTES]).unwrap(),
            "tick" => tick().unwrap(),
            _ => panic!("unknown isolated worker task"),
        }
    }
    #[test]
    fn independent_processes_preserve_error_priority_and_expiry() {
        let _sandbox = crate::test_support::Sandbox::new("process-overlay");
        let pipe = crate::test_support::PanelPipe::new();
        let worker = |task| {
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "overlay::tests::cross_process_worker"])
                .env("G13PAD_OVERLAY_TEST_TASK", task)
                .status()
                .unwrap()
                .success());
        };
        daemon::send_lcd(&vec![61; lcd::BYTES]).unwrap();
        pipe.read();
        worker("error");
        assert_eq!(pipe.read(), error_frame("Cross-process failure").unwrap());
        let expiry = state().unwrap().unwrap().0;
        worker("frame");
        worker("duplicate");
        assert_eq!(
            state().unwrap().unwrap().0,
            expiry,
            "duplicate extended expiry"
        );
        pipe.expect_quiet();
        std::thread::sleep(Duration::from_millis(420));
        worker("tick");
        assert_eq!(pipe.read(), vec![62; lcd::BYTES]);
        assert!(state().unwrap().is_none());
    }
    #[test]
    fn bounded_unicode_errors_and_invalid_cache_recover() {
        let _sandbox = crate::test_support::Sandbox::new("unicode-error");
        let pipe = crate::test_support::PanelPipe::new();
        let large = format!("{}ошибка\n入力\tαβγ", "\0".repeat(1_000_000));
        assert_eq!(error_frame(&large).unwrap().len(), lcd::BYTES);
        fs::write(daemon::runtime_file("frame").unwrap(), vec![0; 2000]).unwrap();
        daemon::send_lcd(&vec![61; lcd::BYTES]).unwrap();
        assert_eq!(pipe.read(), vec![61; lcd::BYTES]);
        notify("ошибка 入力 αβγ").unwrap();
        pipe.read();
        set_enabled(false).unwrap();
        assert_eq!(pipe.read(), vec![61; lcd::BYTES]);
    }
    #[test]
    fn overlay_excludes_animation_and_restores_latest_request() {
        let _sandbox = crate::test_support::Sandbox::new("error-overlay");
        let pipe = crate::test_support::PanelPipe::new();
        daemon::send_lcd(&vec![17; lcd::BYTES]).unwrap();
        assert_eq!(pipe.read(), vec![17; lcd::BYTES]);
        notify_for("Adapter failed", Duration::from_millis(220)).unwrap();
        let error = pipe.read();
        assert_eq!(error.len(), lcd::BYTES);
        assert_ne!(error, vec![17; lcd::BYTES]);
        daemon::send_lcd(&vec![34; lcd::BYTES]).unwrap();
        pipe.expect_quiet();
        std::thread::sleep(Duration::from_millis(100));
        tick().unwrap();
        assert_eq!(pipe.read(), vec![34; lcd::BYTES]);
        notify("Another failure").unwrap();
        pipe.read();
        set_enabled(false).unwrap();
        assert_eq!(pipe.read(), vec![34; lcd::BYTES]);
        notify("Disabled message").unwrap();
        pipe.expect_quiet();
    }
    #[test]
    fn missing_daemon_and_oversized_state_do_not_block_or_write() {
        let _sandbox = crate::test_support::Sandbox::new("error-state");
        notify("Missing driver").unwrap();
        assert!(state().unwrap().is_none());
        fs::write(daemon::runtime_file("overlay").unwrap(), vec![0; 2000]).unwrap();
        let pipe = crate::test_support::PanelPipe::new();
        daemon::send_lcd(&vec![51; lcd::BYTES]).unwrap();
        assert_eq!(pipe.read(), vec![51; lcd::BYTES]);
        assert!(state().unwrap().is_none());
    }
}
