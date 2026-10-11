// SPDX-License-Identifier: GPL-3.0-or-later
//! Transport responsibilities for RetroAchievements.
use super::*;

pub(super) fn root() -> PathBuf {
    crate::state_dir().join("retroarch")
}
pub(super) fn marker() -> PathBuf {
    root().join("current.json")
}

/// PID reuse protection from process metadata.
pub(super) fn process_start(pid: u32) -> Option<String> {
    process_start_in(Path::new("/proc"), pid)
}
pub(super) fn process_start_in(proc_root: &Path, pid: u32) -> Option<String> {
    let stat = fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let tail = stat.rsplit_once(')')?.1;
    if tail.split_whitespace().next()? == "Z" {
        return None;
    }
    tail.split_whitespace().nth(19).map(str::to_string)
}

/// The existing emulator performs its own login. Only recognized diagnostic lines
/// are retained; HTTP bodies, login tokens and unrelated console text are excluded.
/// Private memory pipes must be established here, at process launch. They cannot
/// be attached to an unwrapped RetroArch instance after game detection. The pipe
/// stays idle until a registered game is detected; adapters still validate memory.
pub fn launch(args: &[&str]) -> Result<String, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = root().join(format!("session-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(root(), fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let log_path = dir.join("achievements.log");
    let mut log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&log_path)
        .map_err(|e| e.to_string())?;
    let mut command = Command::new("retroarch");
    command.args(args);
    {
        let config = dir.join("memory.cfg");
        fs::write(
            &config,
            "stdin_cmd_enable = \"true\"\nconfig_save_on_exit = \"false\"\n",
        )
        .map_err(|e| e.to_string())?;
        let appended = core_memory::append_config(args, &config)?;
        command
            .arg("--appendconfig")
            .arg(appended)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
    }
    let mut child = command
        .arg("--verbose")
        // CLI overrides are excluded from RetroArch's saved configuration.
        .arg("--log-file")
        .arg("/dev/stderr")
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot launch RetroArch: {e}"))?;
    let info = serde_json::json!({"pid":child.id(), "start":process_start(child.id()), "session":dir.file_name().unwrap().to_string_lossy()});
    let memory = core_memory::capture(&mut child, dir.clone());
    let tmp = dir.join("current.new");
    let published = (|| -> std::io::Result<()> {
        fs::write(&tmp, info.to_string())?;
        fs::rename(&tmp, marker())
    })();
    // Always drain stderr and reap the child, even if publishing failed.
    let mut pending = Vec::new();
    let mut overflow = false;
    let mut stderr = child.stderr.take().unwrap();
    let mut buf = [0u8; 8192];
    let mut capture_failed = false;
    loop {
        let n = match stderr.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let _ = std::io::stderr().write_all(&buf[..n]);
        for &byte in &buf[..n] {
            if byte == b'\n' {
                if !overflow {
                    if let Ok(line) = std::str::from_utf8(&pending) {
                        memory.observe(line);
                        if event(line).is_some() && writeln!(log, "{line}").is_err() {
                            capture_failed = true;
                        }
                    }
                }
                pending.clear();
                overflow = false;
            } else if pending.len() < MAX_LINE && !overflow {
                pending.push(byte);
            } else {
                pending.clear();
                overflow = true;
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    memory.finish();
    if fs::read(marker()).ok().as_deref() == Some(info.to_string().as_bytes()) {
        let _ = fs::remove_file(marker());
    }
    published.map_err(|e| format!("RetroArch ran, but local capture could not start: {e}"))?;
    if capture_failed {
        return Err("RetroArch ran, but achievement log writes failed".into());
    }
    if !status.success() {
        return Err(format!("RetroArch exited with {status}"));
    }
    Ok("RetroArch session ended".into())
}

#[derive(Default)]
pub(super) struct Tail {
    pub(super) file: Option<File>,
    pub(super) identity: Option<(u64, u64)>,
    pub(super) offset: u64,
    pub(super) pending: Vec<u8>,
    pub(super) overflow: bool,
    pub(super) bootstrap: bool,
    pub(super) dolphin_ready: bool,
}
impl Tail {
    pub(super) fn reset(&mut self, file: File, source: Source) -> std::io::Result<()> {
        let m = file.metadata()?;
        *self = Self {
            file: Some(file),
            identity: Some((m.dev(), m.ino())),
            bootstrap: true,
            offset: if source == Source::Dolphin {
                m.len().saturating_sub(DOLPHIN_HISTORY)
            } else {
                0
            },
            overflow: source == Source::Dolphin && m.len() > DOLPHIN_HISTORY,
            ..Self::default()
        };
        Ok(())
    }
    pub(super) fn poll(
        &mut self,
        path: &PathBuf,
        model: &mut Model,
        source: Source,
        since: Option<u64>,
    ) -> std::io::Result<()> {
        let meta = fs::metadata(path)?;
        if source == Source::Dolphin
            && since.is_none_or(|t| file_millis(&meta).is_none_or(|m| m.saturating_add(2000) < t))
        {
            model.reset();
            return Ok(());
        }
        if self.identity != Some((meta.dev(), meta.ino())) || meta.len() < self.offset {
            self.reset(File::open(path)?, source)?;
            model.reset();
        }
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(BATCH).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        for b in bytes {
            if b == b'\n' {
                if !self.overflow {
                    if let Ok(line) = std::str::from_utf8(&self.pending) {
                        if source == Source::Dolphin
                            && dolphin_message(line) == Some("Achievement Manager Initialized")
                        {
                            self.dolphin_ready =
                                since.is_some_and(|start| dolphin_init_matches(line, start));
                            model.reset();
                        }
                        let e = match source {
                            Source::RetroArch => event(line),
                            Source::Dolphin if self.dolphin_ready => dolphin_event(line),
                            Source::Dolphin => None,
                        };
                        if let Some(e) = e {
                            model.ingest(e, !self.bootstrap);
                        }
                    }
                }
                self.pending.clear();
                self.overflow = false;
            } else if self.pending.len() < MAX_LINE && !self.overflow {
                self.pending.push(b);
            } else {
                self.pending.clear();
                self.overflow = true;
            }
        }
        if self.offset >= meta.len() {
            self.bootstrap = false;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Session {
    pub(super) pid: u32,
    pub(super) start: String,
    pub(super) path: PathBuf,
    pub(super) since: Option<u64>,
}
pub(super) fn session() -> Option<Session> {
    let data = fs::read(marker()).ok()?;
    if data.len() > 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    let pid = u32::try_from(value["pid"].as_u64()?).ok()?;
    let expected = value["start"].as_str()?;
    if process_start(pid).as_deref() != Some(expected) {
        return None;
    }
    let name = value["session"].as_str()?;
    let suffix = name.strip_prefix("session-")?;
    if suffix.is_empty()
        || suffix.len() > 80
        || !suffix.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return None;
    }
    Some(Session {
        pid,
        start: expected.into(),
        path: root().join(name).join("achievements.log"),
        since: None,
    })
}

pub(super) fn native_log_fd(path: &Path, source: Source) -> bool {
    let Some(target) = fs::read_link(path).ok() else {
        return false;
    };
    let Some(name) = target.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let valid_name = match source {
        Source::RetroArch => {
            name == "retroarch.log" || (name.starts_with("retroarch__") && name.ends_with(".log"))
        }
        Source::Dolphin => name == "dolphin.log",
    };
    if !valid_name {
        return false;
    }
    if !fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Some(info) = path
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("fdinfo").join(path.file_name().unwrap()))
    else {
        return false;
    };
    fs::read_to_string(info).ok().is_some_and(|s| {
        s.lines().any(|line| {
            line.strip_prefix("flags:")
                .is_some_and(|flags| u32::from_str_radix(flags.trim(), 8).is_ok_and(|n| n & 3 != 0))
        })
    })
}

/// Match an actual open, writable native log, never a directory's newest old file.
pub(super) fn native_session(proc_root: &Path, source: Source) -> Option<Session> {
    let mut current: Option<Session> = None;
    let mut writers = HashSet::new();
    for process in fs::read_dir(proc_root).ok()?.flatten() {
        let Some(pid) = process.file_name().to_str().and_then(|s| s.parse().ok()) else {
            continue;
        };
        if fs::read_to_string(process.path().join("comm"))
            .ok()
            .as_deref()
            != Some(source.comm())
        {
            continue;
        }
        let Some(start) = process_start_in(proc_root, pid) else {
            continue;
        };
        let Ok(fds) = fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if !native_log_fd(&fd.path(), source) {
                continue;
            }
            writers.insert(pid);
            if current.as_ref().is_none_or(|s| {
                start.parse::<u64>().unwrap_or(0) > s.start.parse::<u64>().unwrap_or(0)
            }) {
                current = Some(Session {
                    pid,
                    start: start.clone(),
                    path: fd.path(),
                    since: if source == Source::Dolphin {
                        process_wall_millis(proc_root, &start)
                    } else {
                        None
                    },
                });
            }
        }
    }
    if source == Source::Dolphin && writers.len() > 1 {
        // Dolphin defaults to one append-only file shared by all instances.
        // Without per-record PIDs, selecting a writer would mix their games.
        None
    } else {
        current
    }
}

pub(super) fn file_millis(meta: &fs::Metadata) -> Option<u64> {
    u64::try_from(
        meta.modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}
pub(super) fn process_wall_millis(proc_root: &Path, start: &str) -> Option<u64> {
    extern "C" {
        fn sysconf(name: std::os::raw::c_int) -> std::os::raw::c_long;
    }
    // Linux _SC_CLK_TCK. This query reads a constant; it has no pointer arguments.
    let ticks = u64::try_from(unsafe { sysconf(2) })
        .ok()
        .filter(|n| *n > 0)?;
    let stat = fs::read_to_string(proc_root.join("stat")).ok()?;
    let boot: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .parse()
        .ok()?;
    boot.checked_mul(1000)?
        .checked_add(start.parse::<u64>().ok()?.checked_mul(1000)? / ticks)
}
pub(super) fn dolphin_init_matches(line: &str, start: u64) -> bool {
    let Some((stamp, _)) = line.split_once(' ') else {
        return false;
    };
    let mut parts = stamp.split(':');
    let Some((minute, second, millis)) = parts
        .next()
        .and_then(number)
        .zip(parts.next().and_then(number))
        .zip(parts.next().and_then(number))
        .map(|((m, s), ms)| (m, s, ms))
    else {
        return false;
    };
    if minute >= 60 || second >= 60 || millis >= 1000 || parts.next().is_some() {
        return false;
    }
    let stamp = u64::from(minute) * 60000 + u64::from(second) * 1000 + u64::from(millis);
    // Dolphin omits hours. Require a fresh file and an initialization close to
    // the live process start, then gate all records on that session boundary.
    // btime is reported in whole seconds; allow two seconds of rounding slack.
    (stamp + 3600000 + 2000 - start % 3600000) % 3600000 <= 122000
}
