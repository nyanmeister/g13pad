// SPDX-License-Identifier: GPL-3.0-or-later
mod actions;
mod client;
mod history;
use client::{value, Client};
use g13pad_music::{request, runtime_dir, Action};
use history::{Store, Tracker};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
        net::UnixListener,
    },
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, String>;
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
fn wall() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
fn db_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("G13MUSIC_DB") {
        return Ok(p.into());
    }
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .ok_or("HOME or XDG_STATE_HOME required")?;
    Ok(root.join("g13music/history.sqlite"))
}
fn private_dir(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| e.to_string())?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{} must be an owned, private directory (0700)",
            path.display()
        ));
    }
    Ok(())
}
fn policy(name: &str, default: f64, max: f64) -> Result<f64> {
    let n = std::env::var(name).map_or(Ok(default), |v| {
        v.parse::<f64>().map_err(|_| format!("invalid {name}"))
    })?;
    if !n.is_finite() || n <= 0. || n > max {
        return Err(format!(
            "{name} must be greater than zero and at most {max}"
        ));
    }
    Ok(n)
}
fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
        libc::signal(libc::SIGTERM, stop as *const () as usize);
        libc::signal(libc::SIGINT, stop as *const () as usize);
    }
    let runtime = runtime_dir()?;
    private_dir(&runtime)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(runtime.join("daemon.lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("g13music is already running in this runtime directory".into());
    }
    let path = db_path()?;
    private_dir(path.parent().ok_or("invalid database path")?)?;
    // The DB lock also prevents two runtime directories from recording twice.
    let db_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path.with_extension("lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(db_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("history database is already owned by a tracker".into());
    }
    let store = Store::open(&path)?;
    let mut tracker = Tracker::new(
        &store,
        policy("G13MUSIC_PLAY_FRACTION", 0.5, 1.)?,
        policy("G13MUSIC_PLAY_SECONDS", 240., 86400.)?,
    )?;
    let socket_path = runtime.join("control.sock");
    if let Err(e) = fs::remove_file(&socket_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.to_string());
        }
    }
    let listener = UnixListener::bind(&socket_path).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut next_tick = Instant::now();
    let mut client = None;
    let mut observed_uri: Option<String> = None;
    let endpoint = g13pad_music::daemon_endpoint()?;
    let mut epoch = 0;
    let mut connection_state = "MPD disconnected".to_owned();
    let mut last_warning = Instant::now() - Duration::from_secs(60);
    eprintln!(
        "g13music {} recording in {}",
        env!("CARGO_PKG_VERSION"),
        path.display()
    );
    while !STOP.load(Ordering::Relaxed) {
        if Instant::now() >= next_tick {
            let result = (|| -> Result<()> {
                if client.is_none() {
                    let mut c = Client::connect()?;
                    let uptime = value(&c.command("stats", &[])?, "uptime")
                        .and_then(|v| v.parse::<i64>().ok())
                        .ok_or("MPD uptime unavailable")?;
                    epoch = wall() - uptime;
                    client = Some(c);
                }
                let c = client.as_mut().unwrap();
                let status = c.status()?;
                let song = c.current()?;
                if status.id != song.as_ref().map(|s| s.id) && !status.stopped {
                    return Err("track changed during observation".into());
                }
                if let Some(s) = &song {
                    actions::ensure(&store, c, s)?;
                }
                observed_uri = song.as_ref().map(|s| s.uri.clone());
                tracker.observe(
                    &store,
                    status,
                    song.as_ref(),
                    start.elapsed().as_secs_f64(),
                    wall(),
                    epoch,
                )?;
                connection_state = "MPD connected".into();
                if let Err(e) = store.project(c) {
                    if !c.recoverable() {
                        return Err(e);
                    }
                    if last_warning.elapsed() >= Duration::from_secs(60) {
                        eprintln!("sticker projection pending: {e}");
                        last_warning = Instant::now();
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                tracker.gap(&store, start.elapsed().as_secs_f64(), wall())?;
                client = None;
                observed_uri = None;
                connection_state = "MPD disconnected".into();
                if last_warning.elapsed() >= Duration::from_secs(60) {
                    eprintln!("MPD observation: {e}");
                    last_warning = Instant::now();
                }
            }
            next_tick = Instant::now() + Duration::from_secs(if client.is_some() { 1 } else { 3 });
        }
        match listener.accept() {
            Ok((mut socket, _)) => {
                let timeout = Some(Duration::from_millis(250));
                socket
                    .set_read_timeout(timeout)
                    .map_err(|e| e.to_string())?;
                socket
                    .set_write_timeout(timeout)
                    .map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                let mut byte = [0u8; 1];
                while bytes.len() < 64 {
                    match socket.read(&mut byte) {
                        Ok(1) if byte[0] != b'\n' => bytes.push(byte[0]),
                        Ok(1) => break,
                        _ => break,
                    }
                }
                let command = std::str::from_utf8(&bytes).unwrap_or("");
                let result = if command == "status" {
                    store.summary().map(|s| format!("{connection_state}; {s}"))
                } else if command == "track" {
                    if let Some(uri) = &observed_uri {
                        store
                            .db
                            .query_row(
                                "SELECT playcount,lastplayed FROM tracks WHERE uri=?1",
                                [uri],
                                |r| {
                                    let playcount = r.get::<_, i64>(0)?;
                                    let lastplayed = r.get::<_, i64>(1)?;
                                    Ok(serde_json::json!({
                                        "uri": uri,
                                        "endpoint": endpoint,
                                        "playcount": playcount,
                                        "lastplayed": lastplayed
                                    })
                                    .to_string())
                                },
                            )
                            .map_err(|e| e.to_string())
                    } else {
                        Err("MPD current track unavailable".into())
                    }
                } else if let Some(action) = Action::parse(command) {
                    tracker.gap(&store, start.elapsed().as_secs_f64(), wall())?;
                    let r = match client.as_mut() {
                        Some(c) => actions::execute(&store, c, action, wall()),
                        None => Err("MPD disconnected".into()),
                    };
                    if r.is_err() && client.as_mut().is_some_and(|c| !c.recoverable()) {
                        client = None;
                    }
                    next_tick = Instant::now();
                    r
                } else {
                    Err("unknown music action".into())
                };
                let (prefix, message) = match result {
                    Ok(s) => ("OK", s),
                    Err(e) => ("ERR", e),
                };
                let message = message.replace(['\n', '\r'], " ");
                let _ = writeln!(socket, "{prefix} {message}");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let mut fd = libc::pollfd {
                    fd: listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let timeout = next_tick
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(1000) as i32;
                unsafe {
                    libc::poll(&mut fd, 1, timeout);
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    tracker.gap(&store, start.elapsed().as_secs_f64(), wall())?;
    fs::remove_file(socket_path).map_err(|e| e.to_string())?;
    Ok(())
}
fn command() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    match args.as_slice() {
        ["--version"] => println!("g13music {}", env!("CARGO_PKG_VERSION")),
        ["run"] => run()?,
        ["status"] => println!("{}", request("status")?),
        ["track"] => println!("{}", request("track")?),
        ["history"] | ["history", _] => {
            let limit = args
                .get(1)
                .map_or(Ok(20usize), |s| {
                    s.parse::<usize>()
                        .map_err(|_| "history limit must be numeric")
                })?
                .clamp(1, 1000);
            let db = rusqlite::Connection::open_with_flags(
                db_path()?,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|e| e.to_string())?;
            let mut query=db.prepare("SELECT s.started,t.title,s.heard_ms,s.counted,t.uri FROM sessions s JOIN tracks t ON t.id=s.track_id ORDER BY s.id DESC LIMIT ?1").map_err(|e|e.to_string())?;
            let rows = query
                .query_map([limit as i64], |r| {
                    Ok(format!(
                        "{}\t{}\t{:.1}s\t{}\t{}",
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)? as f64 / 1000.,
                        if r.get::<_, bool>(3)? {
                            "play"
                        } else {
                            "partial"
                        },
                        r.get::<_, String>(4)?
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                println!("{}", row.map_err(|e| e.to_string())?);
            }
        }
        [action] if Action::parse(action).is_some() => println!("{}", request(action)?),
        [] | ["--help"] => {
            println!(
                "g13music {}\nUsage: g13music run | status | track | history [COUNT] | ACTION\nActions:",
                env!("CARGO_PKG_VERSION")
            );
            for a in Action::ALL {
                println!("  {:18} {}", a.key(), a.label());
            }
        }
        _ => return Err("use g13music --help".into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = command() {
        eprintln!("g13music: {e}");
        std::process::exit(1);
    }
}
