// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only observer for the native Linux EVERSPACE client.
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    os::unix::{
        fs::{FileExt, MetadataExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{self, Command},
    thread,
    time::{Duration, Instant},
};
#[path = "everspace/elf.rs"]
mod elf;
#[path = "everspace/reader.rs"]
mod reader;
use reader::Reader;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn bytes<const N: usize>(file: &File, at: u64) -> io::Result<[u8; N]> {
    let mut b = [0; N];
    file.read_exact_at(&mut b, at)?;
    Ok(b)
}
fn native_game(pid: u32) -> bool {
    fs::read_link(format!("/proc/{pid}/exe"))
        .is_ok_and(|p| p.file_name().is_some_and(|n| n == "RSG-Linux-Shipping"))
}
fn find_game(root: u32) -> Option<u32> {
    let mut todo = vec![root];
    let mut seen = HashSet::new();
    while let Some(pid) = todo.pop() {
        if seen.len() >= 512 || !seen.insert(pid) {
            continue;
        }
        if pid != root && native_game(pid) {
            return Some(pid);
        }
        // Follow only the launched command's tree, including worker-thread children.
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten() {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    todo.extend(
                        children
                            .split_whitespace()
                            .filter_map(|s| s.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    None
}
fn publish(path: &Path, line: &str) -> io::Result<()> {
    fs::create_dir_all(path.parent().ok_or_else(|| invalid("feed has no parent"))?)?;
    let temporary = path.with_extension(format!("everspace-{}.tmp", process::id()));
    fs::write(&temporary, line)?;
    fs::rename(temporary, path)
}
fn default_feed() -> PathBuf {
    env::var_os("G13MAP_SHIP_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
                })
                .join("g13map/ship")
        })
}
fn run() -> io::Result<i32> {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version") {
        println!(
            "g13map-everspace {} (native Linux; tested Steam build 4246486)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(0);
    }
    if args.is_empty() || args[0] == "--help" {
        println!("g13map-everspace [--feed FILE] [--] GAME [ARGS...]\nSteam: g13map-everspace %command%\nRead-only diagnostic: g13map-everspace --pid PID [--once]\nInspect installed executable: g13map-everspace --check-game FILE\nNative Linux x86-64 EVERSPACE; no game writes, debugger attach or mods.\nOther native builds are attempted with validated readings. Telemetry failure never stops the game.");
        return Ok(0);
    }
    if args[0] == "--check-game" {
        if args.len() != 2 {
            return Err(invalid("expected --check-game FILE"));
        }
        let file = File::open(&args[1])?;
        let (_, count) = reader::globals(&file)?;
        println!("Native x86-64 ELF; resolved {count}/6 globals. Member offsets require live validation.");
        return Ok(0);
    }
    if args[0] == "--pid" {
        let pid = args
            .get(1)
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid("expected PID"))?;
        if !(args.len() == 2 || (args.len() == 3 && args[2] == "--once")) {
            return Err(invalid("expected --pid PID [--once]"));
        }
        let observer = Reader::open(pid)?;
        loop {
            print!("{}", observer.snapshot()?);
            io::stdout().flush()?;
            if args.len() == 3 {
                return Ok(0);
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    let mut feed = default_feed();
    if args[0] == "--feed" {
        if args.len() < 3 {
            return Err(invalid("expected --feed FILE GAME [ARGS...]"));
        }
        feed = PathBuf::from(args.remove(1));
        args.remove(0);
    }
    if args.first().is_some_and(|a| a == "--") {
        args.remove(0);
    }
    if args.is_empty() {
        return Err(invalid("game command missing"));
    }
    let mut child = Command::new(&args[0]).args(&args[1..]).spawn()?;
    let mut observer = None;
    let mut next_scan = Instant::now();
    let mut invalid_reads = 0;
    let mut disabled = false;
    let mut warned = false;
    let mut last = String::new();
    let mut written = Instant::now() - Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)));
        }
        if observer.as_ref().is_some_and(Reader::retired) {
            observer = None;
            invalid_reads = 0;
            disabled = false;
            warned = false;
        }
        if observer.is_none() && !disabled && Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(1);
            if let Some(pid) = find_game(process::id()) {
                match Reader::open(pid) {
                    Ok(r) => {
                        eprintln!("g13map-everspace: observing native game PID {pid}");
                        observer = Some(r);
                    }
                    Err(e) if !warned => {
                        eprintln!("g13map-everspace: {e}; game continues without telemetry");
                        warned = true;
                    }
                    Err(_) => {}
                }
            }
        }
        let line = if !disabled {
            observer.as_ref().map(|r| match r.snapshot() {
                Ok(s) => { invalid_reads = 0; s }
                Err(e) => {
                    if e.kind() != io::ErrorKind::WouldBlock { invalid_reads += 1; }
                    if invalid_reads >= 10 {
                        eprintln!("g13map-everspace: telemetry unavailable after repeated invalid readings: {e}; game continues");
                        disabled = true;
                    }
                    "wait ttl 3\n".into()
                }
            }).unwrap_or_else(|| "wait ttl 3\n".into())
        } else {
            "wait ttl 3\n".into()
        };
        if line != last || written.elapsed() >= Duration::from_secs(1) {
            if let Err(e) = publish(&feed, &line) {
                if !warned {
                    eprintln!("g13map-everspace: feed: {e}; game continues");
                    warned = true;
                }
            }
            last = line;
            written = Instant::now();
        }
        thread::sleep(Duration::from_millis(200));
    }
}
fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("g13map-everspace: {e}");
            process::exit(1);
        }
    }
}
