// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests that override process environment serialize here and restore every variable.
use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

static ENV: Mutex<()> = Mutex::new(());

pub struct Sandbox {
    pub dir: PathBuf,
    saved: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

impl Sandbox {
    pub fn new(tag: &str) -> Self {
        let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = env::temp_dir().join(format!("g13map-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut s = Self {
            dir,
            saved: vec![],
            _guard: guard,
        };
        for (key, value) in [
            ("G13MAP_CONFIG", s.dir.join("config")),
            ("G13MAP_PIPE", s.dir.join("pipe")),
            ("G13MAP_OUT_PIPE", s.dir.join("out")),
            ("XDG_RUNTIME_DIR", s.dir.join("runtime")),
            ("G13MAP_ANALOG", PathBuf::from("1")),
            ("G13MAP_ANALOG_MAP", s.dir.join("analog.map")),
            ("G13MAP_UNIT", PathBuf::from("0")),
            ("I3SOCK", s.dir.join("absent-i3.sock")),
        ] {
            s.saved.push((key, env::var_os(key)));
            env::set_var(key, value);
        }
        s
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Captures whole daemon reads on an isolated FIFO, without opening USB or uinput.
pub struct PanelPipe {
    reads: std::sync::mpsc::Receiver<Vec<u8>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PanelPipe {
    pub fn new() -> Self {
        use std::{io::Read, os::unix::fs::OpenOptionsExt, sync::atomic::Ordering};
        let path = crate::daemon::pipe_path();
        assert!(std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success());
        let mut reader = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0o4000)
            .open(path)
            .unwrap();
        let (tx, reads) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut buf = vec![0; 1024 * 1024];
            while !stopped.load(Ordering::Relaxed) {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("FIFO read: {e}"),
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        Self {
            reads,
            stop,
            thread: Some(thread),
        }
    }

    pub fn read(&self) -> Vec<u8> {
        self.reads
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("no panel update")
    }

    pub fn expect_quiet(&self) {
        assert!(
            matches!(
                self.reads
                    .recv_timeout(std::time::Duration::from_millis(150)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "unexpected LCD write"
        );
    }

    #[cfg(feature = "editor")]
    pub fn expect_frames(&self, values: &[u8], count: usize) {
        for _ in 0..count {
            let frame = self.read();
            assert_eq!(frame.len(), crate::lcd::BYTES, "merged frame/command read");
            assert!(values.contains(&frame[0]), "unexpected frame {}", frame[0]);
            assert!(frame.iter().all(|&b| b == frame[0]));
        }
    }
}

impl Drop for PanelPipe {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[cfg(feature = "editor")]
pub fn picture(name: &str, values: &[u8]) {
    use crate::lcd::{Animation, Bitmap, BYTES};
    fs::create_dir_all(crate::lcd::dir()).unwrap();
    fs::write(
        crate::lcd::dir().join(format!("{name}.lpbm")),
        vec![values[0]; BYTES],
    )
    .unwrap();
    if values.len() > 1 {
        let animation = Animation {
            frames: values
                .iter()
                .map(|&v| (std::time::Duration::from_millis(50), Bitmap(vec![v; BYTES])))
                .collect(),
        };
        fs::write(
            crate::lcd::dir().join(format!("{name}.anim")),
            animation.to_bytes(),
        )
        .unwrap();
    }
    crate::save(
        name,
        &crate::profile::Profile::parse(&format!("# lcd {name}\n")).0,
    )
    .unwrap();
}
