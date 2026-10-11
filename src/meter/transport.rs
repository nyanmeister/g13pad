// SPDX-License-Identifier: GPL-3.0-or-later
//! Transport responsibilities for Health Mode.
use super::*;

/// How long a Counter-Strike 2 line lives; the game posts a heartbeat every 5 s.
pub(super) const CS2_TTL: Duration = Duration::from_secs(30);
pub const CS2_PORT: u16 = 3000;

/// The Game State Integration file for `g13map health cs2`: goes into the game's
/// `game/csgo/cfg/` as `gamestate_integration_g13map.cfg`.
pub fn cs2_config(port: u16) -> String {
    format!(
        "\"g13map health\"
{{
  \"uri\" \"http://127.0.0.1:{port}\"
  \"timeout\" \"5.0\"
  \"buffer\" \"0.1\"
  \"throttle\" \"0.1\"
  \"heartbeat\" \"5.0\"
  \"data\"
  {{
    \"provider\" \"1\"
    \"map\" \"1\"
    \"player_id\" \"1\"
    \"player_state\" \"1\"
  }}
}}
"
    )
}

/// What a Counter-Strike 2 game-state post says: the player's health and armour while
/// `player` is the one at the keyboard. Dead, `player` is whoever is being spectated:
/// waiting (asked 2026-10-07; the listener puts a drop to zero in front of it). The
/// menu and the lobby read as full health and no armour (for looks), and a post that is
/// not even JSON as waiting.
pub fn cs2_state(body: &str) -> State {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return State::Wait,
    };
    let me = v["provider"]["steamid"].as_str();
    let player = &v["player"];
    if me.is_some() && player["steamid"].as_str() != me {
        return State::Wait;
    }
    match player["state"]["health"].as_u64() {
        Some(h) => State::Health {
            pct: h.min(999) as u32,
            shield: player["state"]["armor"].as_u64().unwrap_or(0).min(100) as u32,
            helmet: player["state"]["helmet"].as_bool().unwrap_or(false),
            extra: Extra::NONE,
        },
        None => State::health(100),
    }
}

/// Reads one HTTP request and returns its body (up to 1 MB).
pub(super) fn request_body(r: &mut impl Read) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return Err("request header too long".into());
        }
        match r.read(&mut chunk) {
            Ok(0) => return Err("connection closed before the header ended".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e.to_string()),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    if length > 1024 * 1024 {
        return Err("request body too long".into());
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        match r.read(&mut chunk) {
            Ok(0) => return Err("connection closed before the body ended".into()),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(e.to_string()),
        }
    }
    body.truncate(length);
    Ok(String::from_utf8_lossy(&body).to_string())
}

pub(super) fn serve(mut s: TcpStream) -> Result<State, String> {
    s.set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let body = request_body(&mut s)?;
    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    Ok(cs2_state(&body))
}

/// `g13map health cs2`: takes the game's posts on the loopback port and feeds the meter
/// until stopped. The feed carries a TTL, so a closed game or a stopped listener is a
/// meter gone within the TTL.
pub fn cs2(port: u16) -> Result<String, String> {
    cs2_serve(port, &AtomicBool::new(false))
}

/// A reader on a thread of its own: the CS2 listener for `health cs2`, the log
/// follower for `health log`; dropping it stops the thread (and frees the port).
pub struct Listener {
    pub(super) stop: Arc<AtomicBool>,
    pub(super) thread: Option<thread::JoinHandle<()>>,
}

impl Listener {
    /// Follows the game's console log into the feed.
    pub fn follow(path: PathBuf) -> Listener {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || follow_log(&path, &stop))
        };
        Listener {
            stop,
            thread: Some(thread),
        }
    }
    pub fn start(port: u16) -> Listener {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || {
                if let Err(e) = cs2_serve(port, &stop) {
                    eprintln!("health meter: {e}");
                }
            })
        };
        Listener {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Takes the game's posts until `stop` is set.
pub(super) fn cs2_serve(port: u16, stop: &AtomicBool) -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    eprintln!(
        "g13map health cs2: listening on http://127.0.0.1:{port}; the game needs \
         gamestate_integration_g13map.cfg (g13map health cs2-config {port}) in game/csgo/cfg/"
    );
    let mut last: Option<State> = None;
    while !stop.load(Ordering::Relaxed) {
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => {
                eprintln!("g13map health cs2: {e}");
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        let state = match stream
            .set_nonblocking(false)
            .map_err(|e| e.to_string())
            .and_then(|_| serve(stream))
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("g13map health cs2: {e}");
                continue;
            }
        };
        if last != Some(state) {
            eprintln!("g13map health cs2: {state:?}");
            // Alive one post, spectating the next: a death the game did not spell out
            // as zero health. The drop goes first, long enough for the watcher to see it;
            // the meter then holds its dark seconds before showing the wait.
            if state == State::Wait && matches!(last, Some(State::Health { pct, .. }) if pct > 0) {
                write(Some(State::health(0)), Some(CS2_TTL))?;
                thread::sleep(crate::display_clock::TELEMETRY_INTERVAL * 3);
            }
            last = Some(state);
        }
        write(Some(state), Some(CS2_TTL))?;
    }
    Ok(String::new())
}

// ---- a game's console log ----

/// How long a log line lives in the feed; the ACS script repeats itself every ~5 s.
pub(super) const LOG_TTL: Duration = Duration::from_secs(15);

/// The state in a `G13HEALTH HEALTH MAX ARMOR` console line, if the line is one.
pub fn log_line(line: &str) -> Option<State> {
    let rest = line.split("G13HEALTH ").nth(1)?;
    let mut w = rest.split_whitespace();
    let health: f64 = w.next()?.parse().ok()?;
    let max: f64 = w.next()?.parse().ok()?;
    let armor: f64 = w.next()?.parse().ok()?;
    if max <= 0.0 || health < 0.0 || armor < 0.0 {
        return None;
    }
    Some(State::Health {
        pct: (health / max * 100.0).round().clamp(0.0, 999.0) as u32,
        // Doom's green armour is 100, blue 200: the bar thickens past 100.
        shield: armor.round().clamp(0.0, 999.0) as u32,
        helmet: false,
        extra: Extra::NONE,
    })
}

/// The newest file whose name starts with `path`'s (Zandronum appends a timestamp).
pub(super) fn newest_log(path: &std::path::Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_name()?.to_str()?;
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(stem)))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()
        .map(|(_, p)| p)
}

/// Follows the log until `stop`: the last tagged line already there gives the first
/// state, new lines give the rest; a new or truncated file is picked up within a second.
pub(super) fn follow_log(path: &std::path::Path, stop: &AtomicBool) {
    use std::io::{BufRead, Seek, SeekFrom};
    let mut open: Option<(PathBuf, std::io::BufReader<fs::File>, u64)> = None;
    let mut last: Option<State> = None;
    let mut announced = false;
    // The script prints only on change; the feed's ttl is kept alive from here.
    let mut kept = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let newest = newest_log(path);
        if open.as_ref().map(|(p, _, _)| p) != newest.as_ref() {
            open = newest.and_then(|p| {
                let f = fs::File::open(&p).ok()?;
                eprintln!("health meter: following {}", p.display());
                announced = true;
                Some((p, std::io::BufReader::new(f), 0))
            });
            last = None;
        }
        if !announced {
            eprintln!("health meter: no log yet at {}", path.display());
            announced = true;
        }
        let mut changed = None;
        if let Some((p, reader, seen)) = &mut open {
            // Truncated (a new game over the same name): start again from the top.
            if fs::metadata(&*p).map(|m| m.len()).unwrap_or(0) < *seen {
                let _ = reader.seek(SeekFrom::Start(0));
                *seen = 0;
            }
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                *seen += n as u64;
                if let Some(s) = log_line(&line) {
                    changed = Some(s);
                }
                line.clear();
            }
        }
        if changed.is_some() {
            last = changed;
        }
        if let Some(s) = last {
            if changed.is_some() || kept.elapsed() >= LOG_TTL / 3 {
                if let Err(e) = write(Some(s), Some(LOG_TTL)) {
                    eprintln!("health meter: {e}");
                }
                kept = Instant::now();
            }
        }
        thread::sleep(TICK);
    }
}
