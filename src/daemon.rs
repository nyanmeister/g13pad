// SPDX-License-Identifier: GPL-3.0-or-later
//! Talking to g13d: its command FIFO, and whether the analog-stick adapter owns the stick.
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::{
        fs::{FileTypeExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

pub const PIPE: &str = "/run/g13d/g13-0";
/// Exact commands allowed by the existing per-service sudoers rule.
pub const SUDOERS: &str = "%g13 ALL=(root) NOPASSWD: /usr/bin/systemctl start g13-analog.service, /usr/bin/systemctl stop g13-analog.service";
const O_NONBLOCK: i32 = 0o4000; // Linux, all mainstream architectures
/// Largest write a FIFO takes atomically (Linux PIPE_BUF).
const PIPE_BUF: usize = 4096;
/// The daemon takes all available FIFO bytes per read (stock waits on USB first; the custom
/// driver also wakes on FIFO readiness) and takes whatever it
/// holds in one read; a bitmap must be that whole read (exactly 960 bytes). A write fails
/// if the previous one has not drained within this interval: appending would corrupt framing.
const DRAIN: Duration = Duration::from_millis(600);
const LOCK_WAIT: Duration = Duration::from_secs(2);
const FIONREAD: u64 = 0x541B; // Linux, all mainstream architectures

extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

/// Bytes written to the FIFO and not yet read by the daemon. Linux answers FIONREAD on
/// either end of a pipe.
fn unread(f: &File) -> Result<usize, String> {
    let mut n: i32 = 0;
    // SAFETY: FIONREAD writes one int through the pointer, on a valid open descriptor.
    let r = unsafe { ioctl(f.as_raw_fd(), FIONREAD, &mut n as *mut i32) };
    if r == 0 && n >= 0 {
        Ok(n as usize)
    } else {
        Err(format!(
            "cannot inspect G13 FIFO: {}",
            std::io::Error::last_os_error()
        ))
    }
}

/// Waits until the daemon has read everything written before, or `DRAIN` has passed.
fn drained(f: &File) -> Result<(), String> {
    let t = Instant::now();
    while unread(f)? > 0 {
        if t.elapsed() >= DRAIN {
            return Err(
                "g13d FIFO did not drain; refusing to merge commands with an LCD frame".into(),
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// One writer at a time, across processes (the editor, the watcher and `apply` all write):
/// an exclusive lock on a file beside the config, held from the drain wait through the
/// write, so two writers cannot both see the pipe empty and land in the same read.
struct Turn(File);

/// A file of session state named after the pipe (`g13map-g13-0.SUFFIX`), under
/// `$XDG_RUNTIME_DIR` (else the config dir): the writer lock, the editor's lock. Named after
/// the pipe so a test on a stand-in FIFO never shares them with the live session.
pub fn runtime_file(suffix: &str) -> Result<PathBuf, String> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::config_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let pipe = pipe_path();
    let base = pipe.file_name().and_then(|n| n.to_str()).unwrap_or("g13");
    Ok(dir.join(format!("g13map-{base}.{suffix}")))
}

impl Turn {
    fn take() -> Result<Turn, String> {
        let p = runtime_file("lock")?;
        let f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        let t = Instant::now();
        loop {
            match f.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if t.elapsed() < LOCK_WAIT => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        Ok(Turn(f))
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

pub(crate) fn write_atomic(f: &mut File, buf: &[u8]) -> Result<(), String> {
    if buf.len() > PIPE_BUF {
        return Err("G13 FIFO write exceeds PIPE_BUF".into());
    }
    let mut tries = 0;
    loop {
        match f.write(buf) {
            Ok(n) if n == buf.len() => return Ok(()),
            Ok(n) => return Err(format!("short G13 FIFO write: {n}/{} bytes", buf.len())),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && tries < 50 => {
                tries += 1;
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("write to {}: {e}", pipe_path().display())),
        }
    }
}

/// Sends one 960-byte LCD frame (see `lcd`). It goes alone, after the pipe has drained.
pub fn send_lcd(frame: &[u8]) -> Result<(), String> {
    if frame.len() != 960 {
        return Err(format!("LCD frame is {} bytes, not 960", frame.len()));
    }
    lcd_turn(|f| {
        let _ = crate::overlay::remember(frame);
        if !crate::overlay::active().unwrap_or(false) {
            let _ = crate::overlay::clear_expired();
            write_atomic(f, frame)?;
        }
        Ok(())
    })
}

pub(crate) fn lcd_turn<T>(
    operation: impl FnOnce(&mut File) -> Result<T, String>,
) -> Result<T, String> {
    let mut f = open_for_write()?;
    let _turn = Turn::take()?;
    drained(&f)?;
    operation(&mut f)
}

pub fn pipe_path() -> PathBuf {
    env::var_os("G13MAP_PIPE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PIPE.into())
}

/// Never blocks: without a reader the open fails (ENXIO) instead of hanging, which is how a
/// stopped daemon shows up.
fn open_for_write() -> Result<File, String> {
    let path = pipe_path();
    let f = OpenOptions::new()
        .write(true)
        .custom_flags(O_NONBLOCK)
        .open(&path)
        .map_err(|e| match e.raw_os_error() {
            Some(6) => format!(
                "g13d is not reading {} (is g13.service running?)",
                path.display()
            ),
            _ => format!("cannot open {}: {e}", path.display()),
        })?;
    if !f
        .metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_fifo()
    {
        return Err(format!("{} is not a FIFO", path.display()));
    }
    Ok(f)
}

/// Validate the entire payload before opening the FIFO, and preserve every line boundary.
pub(crate) fn command_chunks(commands: &str) -> Result<Vec<Vec<u8>>, String> {
    if commands.contains('\0') {
        return Err("G13 commands contain a NUL byte".into());
    }
    let mut chunks = Vec::new();
    let mut chunk = Vec::new();
    for line in commands.split_inclusive('\n') {
        let newline = usize::from(!line.ends_with('\n'));
        if line.len() + newline > PIPE_BUF {
            return Err("G13 command line exceeds PIPE_BUF".into());
        }
        if chunk.len() + line.len() + newline > PIPE_BUF {
            chunks.push(std::mem::take(&mut chunk));
        }
        chunk.extend_from_slice(line.as_bytes());
        if newline != 0 {
            chunk.push(b'\n');
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    for chunk in &mut chunks {
        // g13d interprets exactly 960 bytes as a bitmap. A blank line is harmless text.
        if chunk.len() == 960 {
            chunk.push(b'\n');
        }
    }
    Ok(chunks)
}

/// Writes command lines to the daemon.
///
/// The installed daemon (g13-git 1e80eda) reads whatever the pipe holds in one go and splits
/// it into lines; a partial line at the end of a read is lost. So every write is atomic: at
/// most PIPE_BUF bytes, cut at a line end.
pub fn send(commands: &str) -> Result<(), String> {
    if commands.trim().is_empty() {
        return Ok(());
    }
    let chunks = command_chunks(commands)?;
    let mut f = open_for_write()?;
    let _turn = Turn::take()?;
    for chunk in chunks {
        drained(&f)?;
        write_atomic(&mut f, &chunk)?;
    }
    Ok(())
}

/// Whether g13d is reading its FIFO. A stale FIFO left by a killed daemon does not count:
/// the daemon holds it O_RDWR, so the non-blocking open fails with ENXIO once it is gone.
pub fn up() -> bool {
    open_for_write().is_ok()
}

/// True while g13-analog.service runs: the stick is in ABSOLUTE mode (an Xbox-style
/// controller) and TOP is its L3 click, so key-mode zones and TOP are not the profile's.
pub fn analog_active() -> bool {
    if let Ok(v) = env::var("G13MAP_ANALOG") {
        return v == "1";
    }
    Command::new("systemctl")
        .args(["is-active", "--quiet", "g13-analog.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Enforces a profile preference through the adapter service, never a raw stickmode line.
/// Hold the FIFO writer lock while its ExecStartPre/ExecStopPost write directly, so our
/// animation threads cannot merge frames with those commands. Missing preferences keep
/// legacy profiles unchanged. The isolated GUI fixture simulates the service with UNIT=0.
pub fn apply_stick(
    mode: Option<crate::profile::StickMode>,
    mapping: Option<crate::gamepad::Mapping>,
) -> Result<bool, String> {
    use crate::profile::StickMode;
    let f = open_for_write()?;
    let _turn = Turn::take()?;
    drained(&f)?;
    let current = analog_active();
    let analog = mode.map_or(current, |mode| mode == StickMode::Analog);
    let wanted = mapping.unwrap_or_default();
    let simulated = env::var("G13MAP_UNIT").is_ok_and(|v| v == "0");
    let map_path = match env::var_os("G13MAP_ANALOG_MAP") {
        Some(path) => PathBuf::from(path),
        None if simulated => runtime_file("analog-map")?,
        None => "/run/g13d/analog.map".into(),
    };
    if simulated && !map_path.exists() {
        fs::write(&map_path, crate::gamepad::Mapping::default().to_text())
            .map_err(|e| e.to_string())?;
    }
    let changed = analog
        && match crate::gamepad::Mapping::read(&map_path) {
            Ok(existing) => existing != wanted,
            Err(_) if !map_path.exists() && wanted == crate::gamepad::Mapping::default() => false,
            Err(e) => {
                return Err(format!(
                    "cannot configure analog mapping: {e}; install the g13pad service files"
                ))
            }
        };
    let state = if env::var_os("G13MAP_ANALOG").is_some() {
        if current {
            "active".to_string()
        } else {
            "inactive".to_string()
        }
    } else {
        let o = Command::new("systemctl")
            .args([
                "show",
                "--property=ActiveState",
                "--value",
                "g13-analog.service",
            ])
            .output()
            .map_err(|e| format!("systemctl: {e}"))?;
        if !o.status.success() {
            return Err(format!(
                "cannot inspect g13-analog.service: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    if ((analog && state == "active") || (!analog && state == "inactive")) && !changed {
        return Ok(analog);
    }
    let control = |verb: &str| -> Result<(), String> {
        let o = Command::new("/usr/bin/timeout")
            .args([
                "15",
                "sudo",
                "-n",
                "/usr/bin/systemctl",
                verb,
                "g13-analog.service",
            ])
            .output()
            .map_err(|e| format!("systemctl {verb}: {e}"))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(format!(
                "cannot {verb} g13-analog.service: {}; sudoers rule: {SUDOERS}",
                String::from_utf8_lossy(&o.stderr).trim()
            ))
        }
    };
    let open_map = || -> Result<File, String> {
        let map = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NONBLOCK | 0o400000 | 0o2000000)
            .open(&map_path)
            .map_err(|e| format!("{}: {e}", map_path.display()))?;
        if !map.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("analog mapping destination is not a regular file".into());
        }
        map.try_lock().map_err(|e| e.to_string())?;
        Ok(map)
    };
    let replace = |map: &mut File, text: &str| -> Result<(), String> {
        map.seek(SeekFrom::Start(0))
            .and_then(|_| map.set_len(0))
            .and_then(|_| map.write_all(text.as_bytes()))
            .map_err(|e| e.to_string())
    };
    // Validate and lock the writable destination before interrupting a working adapter.
    let mut map = changed.then(open_map).transpose()?;
    let previous = if let Some(map) = &mut map {
        let mut text = String::new();
        map.take(513)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        crate::gamepad::Mapping::parse(&text)?;
        Some(text)
    } else {
        None
    };
    let result = (|| {
        if changed {
            if !simulated && state != "inactive" {
                control("stop")?;
                drained(&f)?;
            }
            replace(map.as_mut().unwrap(), &wanted.to_text())?;
        }
        drop(map.take()); // the new adapter must be able to take its shared read lock
        if simulated {
            if env::var_os("G13MAP_ANALOG").is_none() {
                return Err("UNIT=0 simulation requires G13MAP_ANALOG".into());
            }
            env::set_var("G13MAP_ANALOG", if analog { "1" } else { "0" });
        } else {
            control(if analog { "start" } else { "stop" })?;
        }
        drained(&f)?;
        if analog_active() != analog {
            return Err(format!(
                "g13-analog.service did not reach {} mode",
                if analog { "analog" } else { "keys" }
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        drop(map.take());
        if let Some(previous) = previous {
            let rollback = open_map().and_then(|mut map| replace(&mut map, &previous));
            if let Err(rollback) = rollback {
                return Err(format!("{error}; mapping rollback failed: {rollback}"));
            }
            if simulated {
                env::set_var("G13MAP_ANALOG", if current { "1" } else { "0" });
            } else if let Err(rollback) = control(if current { "start" } else { "stop" }) {
                return Err(format!("{error}; adapter recovery failed: {rollback}"));
            }
        }
        return Err(error);
    }
    Ok(analog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn invalid_mapping_destinations_leave_the_adapter_running() {
        let sandbox = crate::test_support::Sandbox::new("bad-map");
        let pipe = crate::test_support::PanelPipe::new();
        let path = sandbox.dir.join("analog.map");
        let wanted = crate::gamepad::Mapping {
            swap: true,
            ..Default::default()
        };
        let check = || {
            assert!(apply_stick(Some(crate::profile::StickMode::Analog), Some(wanted)).is_err());
            assert!(analog_active(), "invalid mapping interrupted adapter");
            pipe.expect_quiet();
        };
        fs::write(&path, "x".repeat(513)).unwrap();
        check();
        fs::remove_file(&path).unwrap();
        let target = sandbox.dir.join("target.map");
        fs::write(&target, crate::gamepad::Mapping::default().to_text()).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        check();
        fs::remove_file(&path).unwrap();
        assert!(Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        check();
        fs::remove_file(&path).unwrap();
        fs::copy(&target, &path).unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        locked.try_lock().unwrap();
        check();
    }

    /// A FIFO with a reader thread; every write must land whole and in order.
    #[test]
    fn long_payloads_are_split_at_line_ends() {
        let sandbox = crate::test_support::Sandbox::new("fifo");
        let dir = &sandbox.dir;
        std::fs::create_dir_all(dir).unwrap();
        let fifo = dir.join("pipe");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        // The reader holds the FIFO O_RDWR like the daemon, so writers never see ENXIO.
        let reader = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let mut long = String::new();
        for i in 0..400 {
            long.push_str(&format!("bind G{i} KEY_LEFTCTRL+KEY_F{i}\n"));
        }
        assert!(long.len() > 2 * PIPE_BUF);
        // Read in a thread: with the account over the kernel's pipe-page soft limit (a Proton
        // game holding thousands of pipes, 2026-09-29) a new FIFO holds one page, and a write
        // longer than that waits for a reader.
        let mut r = reader;
        let n = long.len();
        let reader = thread::spawn(move || {
            let mut got = vec![0u8; n];
            r.read_exact(&mut got).unwrap();
            (r, got)
        });
        send(&long).unwrap();
        let (mut r, got) = reader.join().unwrap();
        assert_eq!(got, long.as_bytes());
        // Exactly 960 bytes gets a blank line so the daemon does not take it for a bitmap.
        let bitmap_sized = format!("out {}", "x".repeat(955));
        send(&bitmap_sized).unwrap();
        let mut got = vec![0u8; 961];
        r.read_exact(&mut got).unwrap();
        assert_eq!(&got[959..], b"\n\n");
        // A frame goes alone and whole, and text after it waits until the frame has been
        // read: the reader takes the frame after a pause, and the text write returns only
        // after that (and well before the DRAIN timeout).
        let reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let mut frame = [0u8; 960];
            r.read_exact(&mut frame).unwrap();
            let mut text = [0u8; 6];
            r.read_exact(&mut text).unwrap();
            (r, frame, text)
        });
        send_lcd(&[7u8; 960]).unwrap();
        let t = Instant::now();
        send("mod 1").unwrap();
        let waited = t.elapsed();
        assert!(
            waited >= Duration::from_millis(200) && waited < DRAIN,
            "text did not wait for the frame to be read: {waited:?}"
        );
        let (r, frame, text) = reader.join().unwrap();
        assert_eq!(frame, [7u8; 960]);
        assert_eq!(&text, b"mod 1\n");
        assert_eq!(unread(&r).unwrap(), 0);
        assert!(send_lcd(&[0u8; 10]).is_err());
        assert!(up());
        drop(r);
        assert!(!up());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stalled_reader_must_not_merge_frames_and_commands() {
        let sandbox = crate::test_support::Sandbox::new("stalled");
        let fifo = sandbox.dir.join("pipe");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let mut reader = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        send_lcd(&[7; 960]).unwrap();
        assert!(
            send("mod 1").is_err(),
            "text was appended to an unread LCD frame"
        );
        assert_eq!(unread(&reader).unwrap(), 960);
        let mut frame = [0; 960];
        reader.read_exact(&mut frame).unwrap();
        assert_eq!(frame, [7; 960]);
        assert!(send("out hello\0bind G1 KEY_A").is_err());
        assert!(send(&format!("out {}", "x".repeat(4096))).is_err());
        assert_eq!(unread(&reader).unwrap(), 0);
        let turn = Turn::take().unwrap();
        let t = Instant::now();
        assert!(thread::spawn(|| send("mod 2")).join().unwrap().is_err());
        assert!(t.elapsed() < LOCK_WAIT + Duration::from_secs(1));
        assert_eq!(unread(&reader).unwrap(), 0);
        drop(turn);
        send("mod 2").unwrap();
        let mut command = [0; 6];
        reader.read_exact(&mut command).unwrap();
        assert_eq!(&command, b"mod 2\n");
        drop(reader);
        fs::remove_file(&fifo).unwrap();
        fs::write(&fifo, "regular file").unwrap();
        assert!(send("mod 3").is_err());
        assert_eq!(fs::read_to_string(&fifo).unwrap(), "regular file");
    }

    #[test]
    fn concurrent_writers_keep_frames_and_command_reads_separate() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let sandbox = crate::test_support::Sandbox::new("concurrent");
        let fifo = sandbox.dir.join("pipe");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let mut reader = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let reader = thread::spawn(move || {
            let mut buf = [0; 65536];
            let mut frames = 0;
            let mut lines = 0;
            while !done.load(Ordering::Relaxed) || unread(&reader).unwrap() > 0 {
                match reader.read(&mut buf) {
                    Ok(960) => {
                        assert!(buf[..960].iter().all(|&b| b == 0x85));
                        frames += 1;
                    }
                    Ok(n) if n > 0 => {
                        assert_eq!(buf[n - 1], b'\n');
                        for line in std::str::from_utf8(&buf[..n]).unwrap().lines() {
                            assert_eq!(line, "bind G1 KEY_A");
                            lines += 1;
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("FIFO read: {e}"),
                }
                thread::sleep(Duration::from_millis(3));
            }
            (frames, lines)
        });
        let writers: Vec<_> = (0..3)
            .map(|_| {
                thread::spawn(|| {
                    for _ in 0..10 {
                        send_lcd(&[0x85; 960]).unwrap();
                        // Two atomic writes per send; the reader must see whole command lines.
                        send(&"bind G1 KEY_A\n".repeat(400)).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        assert_eq!(reader.join().unwrap(), (30, 12000));
    }
}
