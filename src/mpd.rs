// SPDX-License-Identifier: GPL-3.0-or-later
//! Native readonly MPD transport and legacy statistics; playback owns the LCD.
mod playback;
#[cfg(test)]
use crate::lcd::{Bitmap, BYTES, W};
pub use playback::Live;
#[cfg(test)]
use std::thread;
use std::{
    ffi::{c_char, c_int, c_uint, c_void, CStr, CString},
    ptr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// libmpdclient owns protocol parsing, bounded input buffers and socket timeouts.
// Every handle and borrowed pair stays on its creating thread.
#[repr(C)]
struct Pair {
    name: *const c_char,
    value: *const c_char,
}
#[link(name = "mpdclient")]
unsafe extern "C" {
    fn mpd_settings_new(
        host: *const c_char,
        port: c_uint,
        timeout: c_uint,
        reserved: *const c_char,
        password: *const c_char,
    ) -> *mut c_void;
    fn mpd_settings_free(settings: *mut c_void);
    fn mpd_settings_get_host(settings: *const c_void) -> *const c_char;
    fn mpd_settings_get_port(settings: *const c_void) -> c_uint;
    fn mpd_settings_get_password(settings: *const c_void) -> *const c_char;
    fn mpd_connection_new(host: *const c_char, port: c_uint, timeout: c_uint) -> *mut c_void;
    fn mpd_connection_get_fd(connection: *const c_void) -> c_int;
    fn mpd_connection_free(connection: *mut c_void);
    fn mpd_connection_get_error(connection: *const c_void) -> c_int;
    fn mpd_connection_get_server_error(connection: *const c_void) -> c_int;
    fn mpd_connection_set_timeout(connection: *mut c_void, timeout: c_uint);
    fn mpd_run_password(connection: *mut c_void, password: *const c_char) -> bool;
    fn mpd_send_stats(connection: *mut c_void) -> bool;
    fn mpd_send_status(connection: *mut c_void) -> bool;
    fn mpd_recv_pair(connection: *mut c_void) -> *mut Pair;
    fn mpd_return_pair(connection: *mut c_void, pair: *mut Pair);
    fn mpd_response_finish(connection: *mut c_void) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Error {
    Offline,
    Denied,
    Address,
}
impl Error {
    fn label(self) -> &'static str {
        match self {
            Self::Offline => "MPD DISCONNECTED",
            Self::Denied => "MPD ACCESS DENIED",
            Self::Address => "MPD ADDRESS INVALID",
        }
    }
}
struct Settings(*mut c_void);
impl Drop for Settings {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { mpd_settings_free(self.0) }
        }
    }
}
struct Client(*mut c_void);
impl Drop for Client {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { mpd_connection_free(self.0) }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Stats {
    uptime: Option<u64>,
    played: Option<u64>,
    songs: Option<u64>,
    albums: Option<u64>,
    artists: Option<u64>,
    library: Option<u64>,
    updated: Option<u64>,
    updating: bool,
}
impl Stats {
    fn feed(&mut self, key: &str, value: &str) {
        let number = value.parse::<u64>().ok();
        match key {
            "uptime" => self.uptime = number,
            "playtime" => self.played = number,
            "songs" => self.songs = number,
            "albums" => self.albums = number,
            "artists" => self.artists = number,
            "db_playtime" => self.library = number,
            "db_update" => self.updated = number.filter(|n| *n != 0),
            "updating_db" => self.updating = number.is_some_and(|n| n != 0),
            _ => {}
        }
    }
}
impl Client {
    fn connect() -> Result<Self, Error> {
        // Settings handle MPD_HOST/MPD_PORT and password@address escaping.
        let settings = Settings(unsafe {
            mpd_settings_new(
                if std::env::var_os("MPD_HOST").is_some() {
                    ptr::null()
                } else {
                    c"127.0.0.1".as_ptr()
                },
                0,
                300,
                ptr::null(),
                ptr::null(),
            )
        });
        if settings.0.is_null() {
            return Err(Error::Offline);
        }
        let host = unsafe { mpd_settings_get_host(settings.0) };
        let host = if host.is_null() {
            "localhost"
        } else {
            unsafe { CStr::from_ptr(host) }
                .to_str()
                .map_err(|_| Error::Address)?
        };
        // Numeric addresses and sockets avoid an unbounded DNS lookup when a profile
        // stops. localhost is the normal MPD default, including this machine.
        let host = if host == "localhost" {
            "127.0.0.1"
        } else {
            host
        };
        if !host.starts_with('/')
            && !host.starts_with('@')
            && host.parse::<std::net::IpAddr>().is_err()
        {
            return Err(Error::Address);
        }
        let host = CString::new(host).map_err(|_| Error::Address)?;
        let client = Self(unsafe {
            mpd_connection_new(host.as_ptr(), mpd_settings_get_port(settings.0), 300)
        });
        if client.0.is_null() {
            return Err(Error::Offline);
        }
        client.check()?;
        let password = unsafe { mpd_settings_get_password(settings.0) };
        if !password.is_null() && !unsafe { mpd_run_password(client.0, password) } {
            return Err(Error::Denied);
        }
        Ok(client)
    }
    fn check(&self) -> Result<(), Error> {
        match unsafe { mpd_connection_get_error(self.0) } {
            0 => Ok(()),
            9 if unsafe { mpd_connection_get_server_error(self.0) } == 4 => Err(Error::Denied),
            _ => Err(Error::Offline),
        }
    }

    fn process_id(&self) -> Option<u32> {
        g13pad_music::peer_process_id(unsafe { mpd_connection_get_fd(self.0) })
    }
    fn pairs(
        &mut self,
        send: unsafe extern "C" fn(*mut c_void) -> bool,
    ) -> Result<Vec<(String, String)>, Error> {
        let deadline = Instant::now() + Duration::from_millis(300);
        self.timeout(deadline)?;
        if !unsafe { send(self.0) } {
            return self.check().and(Err(Error::Offline));
        }
        let mut result = Vec::new();
        let mut bytes = 0;
        loop {
            self.timeout(deadline)?;
            let p = unsafe { mpd_recv_pair(self.0) };
            if p.is_null() {
                break;
            }
            let pair = unsafe {
                (
                    CStr::from_ptr((*p).name).to_str().map(str::to_owned),
                    CStr::from_ptr((*p).value).to_str().map(str::to_owned),
                )
            };
            unsafe { mpd_return_pair(self.0, p) };
            let (k, v) = (
                pair.0.map_err(|_| Error::Offline)?,
                pair.1.map_err(|_| Error::Offline)?,
            );
            bytes += k.len() + v.len();
            if bytes > 65536 || result.len() >= 128 {
                return Err(Error::Offline);
            }
            result.push((k, v));
        }
        self.timeout(deadline)?;
        if !unsafe { mpd_response_finish(self.0) } {
            return self.check().and(Err(Error::Offline));
        }
        Ok(result)
    }

    fn sample(&mut self) -> Result<Stats, Error> {
        let mut stats = Stats::default();
        // A single deadline covers both responses; malformed/flooding peers cannot
        // extend a profile handoff indefinitely. Never publish a partial snapshot.
        let deadline = Instant::now() + Duration::from_millis(600);
        for send in [
            mpd_send_stats as unsafe extern "C" fn(*mut c_void) -> bool,
            mpd_send_status,
        ] {
            self.timeout(deadline)?;
            if !unsafe { send(self.0) } {
                return self.check().and(Err(Error::Offline));
            }
            let mut complete = false;
            for _ in 0..128 {
                self.timeout(deadline)?;
                let pair = unsafe { mpd_recv_pair(self.0) };
                if pair.is_null() {
                    complete = true;
                    break;
                }
                // Strings are borrowed only until mpd_return_pair; no pointer escapes.
                unsafe {
                    let key = CStr::from_ptr((*pair).name).to_str();
                    let value = CStr::from_ptr((*pair).value).to_str();
                    if let (Ok(key), Ok(value)) = (key, value) {
                        stats.feed(key, value);
                    }
                    mpd_return_pair(self.0, pair);
                }
            }
            if !complete {
                return Err(Error::Offline);
            }
            self.timeout(deadline)?;
            if !unsafe { mpd_response_finish(self.0) } {
                return self.check().and(Err(Error::Offline));
            }
        }
        self.check()?;
        Ok(stats)
    }
    fn timeout(&self, deadline: Instant) -> Result<(), Error> {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        if remaining == 0 {
            return Err(Error::Offline);
        }
        unsafe {
            mpd_connection_set_timeout(self.0, remaining as c_uint);
        }
        Ok(())
    }
}

fn duration(seconds: Option<u64>) -> String {
    match seconds {
        None => "--".into(),
        Some(s) if s >= 86400 => format!("{}D{}H", s / 86400, s / 3600 % 24),
        Some(s) if s >= 3600 => format!("{}H{}M", s / 3600, s / 60 % 60),
        Some(s) => format!("{}M{}S", s / 60, s % 60),
    }
}
fn count(value: Option<u64>) -> String {
    match value {
        None => "--".into(),
        Some(v) if v < 1_000_000 => v.to_string(),
        Some(v) => format!("{:.1}M", v as f64 / 1_000_000.0),
    }
}
fn rows(stats: &Stats, now: u64) -> [String; 5] {
    let age = stats.updated.and_then(|stamp| now.checked_sub(stamp));
    [
        format!("MPD UP {}", duration(stats.uptime)),
        format!("PLAYED {}", duration(stats.played)),
        format!(
            "SONGS {}|ALBUMS {}",
            count(stats.songs),
            count(stats.albums)
        ),
        format!(
            "ARTISTS {}|LIB {}",
            count(stats.artists),
            duration(stats.library)
        ),
        if stats.updating {
            "DB SCANNING".into()
        } else {
            format!("DB AGE {}", duration(age))
        },
    ]
}
#[cfg(test)]
fn render(stats: Result<&Stats, Error>, now: u64) -> [u8; BYTES] {
    let mut bm = Bitmap::blank();
    let lines = match stats {
        Ok(stats) => rows(stats, now),
        Err(e) => [
            e.label().into(),
            "WAITING FOR DAEMON".into(),
            String::new(),
            String::new(),
            String::new(),
        ],
    };
    for (row, line) in lines.iter().enumerate() {
        let (left, right) = line.split_once('|').unwrap_or((line, ""));
        // Each value remains in its own bounded column. Extreme counts/durations
        // show a dash rather than painting across the neighbouring measurement.
        let limit = if right.is_empty() { 26 } else { 13 };
        let left = if left.len() > limit { "--" } else { left };
        let right = if right.len() > 12 { "--" } else { right };
        crate::meter::text(&mut bm, 0, 2 + row as i32 * 8, left, 1);
        crate::meter::text(
            &mut bm,
            W as i32 - right.len() as i32 * 6 + 1,
            2 + row as i32 * 8,
            right,
            1,
        );
    }
    bm.0.try_into().unwrap()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn command(args: &[&str]) -> Result<String, String> {
    if !matches!(args, ["status"] | ["preview", _]) {
        return Err("usage: g13map mpd status | mpd preview FILE.lpbm".into());
    }
    if let ["preview", file] = args {
        return playback::preview(file);
    }
    let mut client = Client::connect().map_err(|e| e.label().to_string())?;
    let stats = client.sample().map_err(|e| e.label().to_string())?;
    match args {
        ["status"] => Ok(rows(&stats, now()).join("\n").replace('|', "   ")),
        ["preview", file] => playback::preview(file),
        _ => Err("usage: g13map mpd status | mpd preview FILE.lpbm".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_invalid_values_are_distinct_from_zero() {
        let mut s = Stats::default();
        s.feed("songs", "0");
        s.feed("albums", "-1");
        s.feed("artists", "broken");
        s.feed("db_update", "0");
        s.feed("db_playtime", "18446744073709551616");
        assert_eq!(rows(&s, 100)[2], "SONGS 0|ALBUMS --");
        assert_eq!(rows(&s, 100)[3], "ARTISTS --|LIB --");
        assert_eq!(rows(&s, 100)[4], "DB AGE --");
        s.feed("db_update", "101");
        assert_eq!(rows(&s, 100)[4], "DB AGE --");
        s.feed("updating_db", "7");
        assert_eq!(rows(&s, 100)[4], "DB SCANNING");
    }
    #[test]
    fn durations_and_large_counters_fit_the_screen() {
        assert_eq!(duration(Some(175 * 86400 + 20 * 3600)), "175D20H");
        assert_eq!(duration(Some(19834)), "5H30M");
        assert_eq!(count(Some(28795)), "28795");
        assert_eq!(count(Some(1_250_000)), "1.2M");
        let s = Stats {
            uptime: Some(u64::MAX),
            played: Some(u64::MAX),
            songs: Some(u64::MAX),
            albums: Some(u64::MAX),
            artists: Some(u64::MAX),
            library: Some(u64::MAX),
            ..Stats::default()
        };
        assert_eq!(render(Ok(&s), 0).len(), BYTES);
        assert_ne!(
            render(Ok(&Stats::default()), 0),
            render(Err(Error::Offline), 0)
        );
    }
    // Exercise the actual native protocol client with a private socket. Only
    // stats/status are accepted; any accidental playback command fails the test.
    fn peer(
        listener: std::os::unix::net::UnixListener,
        response: &'static str,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream.write_all(b"OK MPD 0.24.0\n").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut command = String::new();
                if reader.read_line(&mut command).unwrap() == 0 {
                    break;
                }
                match command.as_str() {
                    "stats\n" => stream.write_all(response.as_bytes()).unwrap(),
                    "currentsong\n" => stream.write_all(b"OK\n").unwrap(),
                    "status\n" => stream
                        .write_all(b"state: stop\nupdating_db: 7\nOK\n")
                        .unwrap(),
                    _ => panic!("unexpected MPD command: {command}"),
                }
            }
        })
    }

    #[test]
    fn native_protocol_preserves_missing_values_and_reconnects() {
        let sandbox = crate::test_support::Sandbox::new("mpd-protocol");
        let path = sandbox.dir.join("mpd.socket");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let worker = peer(listener, "songs: 28795\nalbums: 1180\nartists: 4240\nuptime: 100\nplaytime: 0\ndb_update: 0\ndb_playtime: bad\nOK\n");
        let mut client = Client::connect().unwrap();
        let stats = client.sample().unwrap();
        assert_eq!(stats.songs, Some(28795));
        assert_eq!(stats.played, Some(0));
        assert_eq!(stats.library, None);
        assert_eq!(stats.updated, None);
        assert!(stats.updating);
        drop(client);
        worker.join().unwrap();
        assert!(matches!(Client::connect(), Err(Error::Offline)));
        std::fs::remove_file(&path).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let worker = peer(listener, "songs: 1\nuptime: 1\nplaytime: 0\nOK\n");
        let mut client = Client::connect().unwrap();
        assert_eq!(client.sample().unwrap().songs, Some(1));
        drop(client);
        worker.join().unwrap();
    }

    #[test]
    fn stalled_server_and_denied_responses_discard_partial_statistics() {
        use std::io::{BufRead, BufReader, Write};
        let sandbox = crate::test_support::Sandbox::new("mpd-failure");
        let path = sandbox.dir.join("mpd.socket");
        for deny in [false, true] {
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            let worker = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.write_all(b"OK MPD 0.24.0\n").unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                assert_eq!(line, "stats\n");
                if deny {
                    stream
                        .write_all(b"ACK [4@0] {stats} permission denied\n")
                        .unwrap();
                } else {
                    stream.write_all(b"songs: 999\n").unwrap();
                    thread::sleep(Duration::from_millis(900));
                }
            });
            let mut client = Client::connect().unwrap();
            let start = Instant::now();
            assert_eq!(
                client.sample(),
                Err(if deny { Error::Denied } else { Error::Offline })
            );
            assert!(start.elapsed() < Duration::from_millis(850));
            drop(client);
            worker.join().unwrap();
            std::fs::remove_file(&path).unwrap();
        }
    }

    #[test]
    fn collector_yields_to_editor_and_stops_before_next_profile() {
        let sandbox = crate::test_support::Sandbox::new("mpd-editor");
        let pipe = crate::test_support::PanelPipe::new();
        let listener =
            std::os::unix::net::UnixListener::bind(sandbox.dir.join("mpd.socket")).unwrap();
        let worker = peer(listener, "songs: 28795\nalbums: 1180\nartists: 4240\nuptime: 345600\nplaytime: 19834\ndb_playtime: 15194277\nOK\n");
        let live = Live::start();
        let frame = pipe.read();
        assert_eq!(frame.len(), BYTES);
        let editor = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(crate::daemon::runtime_file("editor").unwrap())
            .unwrap();
        editor
            .try_lock()
            .expect("MPD prevented the editor from opening");
        thread::sleep(Duration::from_millis(100));
        pipe.expect_quiet();
        drop(editor);
        let resumed = pipe.read();
        assert_eq!(resumed.len(), BYTES);
        // The async Unicode row may replace the initial fallback while the editor
        // is open. The stable playback rows must survive the ownership handoff.
        let before = Bitmap::from_lpbm(&frame).unwrap();
        let after = Bitmap::from_lpbm(&resumed).unwrap();
        for y in 11..43 {
            for x in 0..W {
                assert_eq!(before.get(x, y), after.get(x, y));
            }
        }
        let start = Instant::now();
        drop(live);
        assert!(start.elapsed() < Duration::from_secs(1));
        worker.join().unwrap();
        pipe.expect_quiet();
    }
}
