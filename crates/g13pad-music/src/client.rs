// SPDX-License-Identifier: GPL-3.0-or-later
//! libmpdclient owns escaping, authentication, framing and bounded socket buffers.
use std::{
    ffi::{c_char, c_int, c_uint, c_void, CStr, CString},
    ptr,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
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
    fn mpd_settings_free(p: *mut c_void);
    fn mpd_settings_get_host(p: *const c_void) -> *const c_char;
    fn mpd_settings_get_port(p: *const c_void) -> c_uint;
    fn mpd_settings_get_password(p: *const c_void) -> *const c_char;
    fn mpd_connection_new(host: *const c_char, port: c_uint, timeout: c_uint) -> *mut c_void;
    fn mpd_connection_free(p: *mut c_void);
    fn mpd_connection_get_error(p: *const c_void) -> c_int;
    fn mpd_connection_get_error_message(p: *const c_void) -> *const c_char;
    fn mpd_connection_clear_error(p: *mut c_void) -> bool;
    fn mpd_connection_set_timeout(p: *mut c_void, timeout: c_uint);
    fn mpd_run_password(p: *mut c_void, password: *const c_char) -> bool;
    fn mpd_send_command(p: *mut c_void, command: *const c_char, ...) -> bool;
    fn mpd_recv_pair(p: *mut c_void) -> *mut Pair;
    fn mpd_return_pair(p: *mut c_void, pair: *mut Pair);
    fn mpd_response_finish(p: *mut c_void) -> bool;
    fn mpd_command_list_begin(p: *mut c_void, discrete: bool) -> bool;
    fn mpd_command_list_end(p: *mut c_void) -> bool;
}
struct Settings(*mut c_void);
impl Drop for Settings {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { mpd_settings_free(self.0) }
        }
    }
}
pub struct Client(*mut c_void);
impl Drop for Client {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { mpd_connection_free(self.0) }
        }
    }
}
impl Client {
    pub fn connect() -> Result<Self> {
        let settings = Settings(unsafe {
            mpd_settings_new(
                if std::env::var_os("MPD_HOST").is_some() {
                    ptr::null()
                } else {
                    c"127.0.0.1".as_ptr()
                },
                0,
                1000,
                ptr::null(),
                ptr::null(),
            )
        });
        if settings.0.is_null() {
            return Err("MPD settings unavailable".into());
        }
        let host = unsafe { mpd_settings_get_host(settings.0) };
        if host.is_null() {
            return Err("MPD address unavailable".into());
        }
        let host = unsafe { CStr::from_ptr(host) }
            .to_str()
            .map_err(|_| "invalid MPD address")?;
        let host = if host == "localhost" {
            "127.0.0.1"
        } else {
            host
        };
        if !host.starts_with('/')
            && !host.starts_with('@')
            && host.parse::<std::net::IpAddr>().is_err()
        {
            return Err("use localhost, a numeric MPD address or an absolute Unix socket".into());
        }
        let host = CString::new(host).map_err(|e| e.to_string())?;
        let client = Self(unsafe {
            mpd_connection_new(host.as_ptr(), mpd_settings_get_port(settings.0), 1000)
        });
        if client.0.is_null() {
            return Err("MPD connection allocation failed".into());
        }
        client.check()?;
        let password = unsafe { mpd_settings_get_password(settings.0) };
        if !password.is_null() && !unsafe { mpd_run_password(client.0, password) } {
            return Err("MPD authentication failed".into());
        }
        Ok(client)
    }
    fn check(&self) -> Result<()> {
        if unsafe { mpd_connection_get_error(self.0) } == 0 {
            return Ok(());
        }
        let p = unsafe { mpd_connection_get_error_message(self.0) };
        // Library diagnostics never include the MPD_HOST credential string.
        Err(if p.is_null() {
            "MPD command failed".into()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        })
    }
    pub fn recoverable(&mut self) -> bool {
        unsafe { mpd_connection_clear_error(self.0) }
    }
    fn send(&self, command: &str, args: &[&str]) -> Result<()> {
        let command = CString::new(command).map_err(|e| e.to_string())?;
        let a = args
            .iter()
            .map(|a| CString::new(*a).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>>>()?;
        let nil = ptr::null::<c_char>();
        // The variadic API requires an explicitly typed null terminator.
        let sent = unsafe {
            match a.len() {
                0 => mpd_send_command(self.0, command.as_ptr(), nil),
                1 => mpd_send_command(self.0, command.as_ptr(), a[0].as_ptr(), nil),
                2 => mpd_send_command(self.0, command.as_ptr(), a[0].as_ptr(), a[1].as_ptr(), nil),
                3 => mpd_send_command(
                    self.0,
                    command.as_ptr(),
                    a[0].as_ptr(),
                    a[1].as_ptr(),
                    a[2].as_ptr(),
                    nil,
                ),
                4 => mpd_send_command(
                    self.0,
                    command.as_ptr(),
                    a[0].as_ptr(),
                    a[1].as_ptr(),
                    a[2].as_ptr(),
                    a[3].as_ptr(),
                    nil,
                ),
                5 => mpd_send_command(
                    self.0,
                    command.as_ptr(),
                    a[0].as_ptr(),
                    a[1].as_ptr(),
                    a[2].as_ptr(),
                    a[3].as_ptr(),
                    a[4].as_ptr(),
                    nil,
                ),
                _ => return Err("too many MPD arguments".into()),
            }
        };
        if sent {
            Ok(())
        } else {
            self.check().and(Err("MPD send failed".into()))
        }
    }
    pub fn command(&mut self, command: &str, args: &[&str]) -> Result<Vec<(String, String)>> {
        unsafe { mpd_connection_set_timeout(self.0, 1000) };
        self.send(command, args)?;
        self.receive()
    }
    fn receive(&self) -> Result<Vec<(String, String)>> {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut pairs = Vec::new();
        let mut bytes = 0usize;
        loop {
            if Instant::now() >= deadline || pairs.len() >= 300_000 || bytes > 32 * 1024 * 1024 {
                return Err("MPD response exceeded bounded collection limits".into());
            }
            unsafe {
                mpd_connection_set_timeout(
                    self.0,
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(1000) as c_uint,
                )
            };
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
            let (key, value) = (
                pair.0.map_err(|_| "invalid MPD UTF-8")?,
                pair.1.map_err(|_| "invalid MPD UTF-8")?,
            );
            bytes += key.len() + value.len();
            pairs.push((key, value));
        }
        if !unsafe { mpd_response_finish(self.0) } {
            return self.check().and(Err("incomplete MPD response".into()));
        }
        self.check()?;
        Ok(pairs)
    }
    pub fn reorder(&mut self, swaps: &[(u32, u32)]) -> Result<()> {
        // Bounded command lists allow server input-buffer limits even on large queues.
        for chunk in swaps.chunks(200) {
            if !unsafe { mpd_command_list_begin(self.0, false) } {
                return Err("MPD command list failed".into());
            }
            for &(a, b) in chunk {
                self.send("swapid", &[&a.to_string(), &b.to_string()])?;
            }
            if !unsafe { mpd_command_list_end(self.0) } {
                return Err("MPD command list failed".into());
            }
            self.receive()?;
        }
        Ok(())
    }
    pub fn status(&mut self) -> Result<Status> {
        Ok(Status::new(&self.command("status", &[])?))
    }
    pub fn current(&mut self) -> Result<Option<Song>> {
        Ok(songs(self.command("currentsong", &[])?)?.pop())
    }
    pub fn queue(&mut self) -> Result<Vec<Song>> {
        songs(self.command("playlistinfo", &[])?)
    }
    pub fn sticker(&mut self, uri: &str, name: &str) -> Result<Option<String>> {
        // list returns OK with no pairs for songs with no stickers; get would ACK.
        let prefix = format!("{name}=");
        Ok(self
            .command("sticker", &["list", "song", uri])?
            .into_iter()
            .find_map(|(k, v)| {
                (k == "sticker")
                    .then(|| v.strip_prefix(&prefix).map(str::to_owned))
                    .flatten()
            }))
    }
    pub fn set_sticker(&mut self, uri: &str, name: &str, value: &str) -> Result<()> {
        self.command("sticker", &["set", "song", uri, name, value])?;
        Ok(())
    }
}
pub fn value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub playing: bool,
    pub stopped: bool,
    pub id: Option<u32>,
    pub elapsed: f64,
    pub queue_version: u64,
    pub random: bool,
}
impl Status {
    fn new(p: &[(String, String)]) -> Self {
        Self {
            playing: value(p, "state") == Some("play"),
            stopped: value(p, "state") == Some("stop"),
            id: value(p, "songid").and_then(|v| v.parse().ok()),
            elapsed: value(p, "elapsed")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.)
                .unwrap_or(0.),
            queue_version: value(p, "playlist")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            random: value(p, "random") == Some("1"),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Song {
    pub uri: String,
    pub id: u32,
    pub duration: f64,
    pub album: String,
    pub artist: String,
    pub title: String,
    pub album_key: String,
    pub disc: u32,
    pub track: u32,
}
fn number(s: Option<&str>) -> u32 {
    s.and_then(|v| v.split('/').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
fn songs(pairs: Vec<(String, String)>) -> Result<Vec<Song>> {
    let mut result = Vec::new();
    let mut start = 0;
    for i in 1..=pairs.len() {
        if i == pairs.len() || pairs[i].0 == "file" {
            let p = &pairs[start..i];
            start = i;
            if let Some(uri) = value(p, "file") {
                if result.len() >= 20_000 {
                    return Err("queue exceeds 20,000-track limit".into());
                }
                let album = value(p, "Album").unwrap_or("").to_owned();
                let artist = value(p, "AlbumArtist")
                    .or_else(|| value(p, "Artist"))
                    .unwrap_or("")
                    .to_owned();
                // No album tag: a path's parent keeps unrelated loose tracks separate.
                let album_key = if let Some(id) = value(p, "MUSICBRAINZ_ALBUMID") {
                    format!("mb:{id}")
                } else if album.is_empty() {
                    format!("dir:{}", uri.rsplit_once('/').map_or(uri, |(d, _)| d))
                } else {
                    format!("{:?}", (&artist, &album, value(p, "Date").unwrap_or("")))
                };
                result.push(Song {
                    uri: uri.into(),
                    id: number(value(p, "Id")),
                    duration: value(p, "duration")
                        .or_else(|| value(p, "Time"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v > 0.)
                        .unwrap_or(0.),
                    album,
                    artist,
                    title: value(p, "Title").unwrap_or(uri).into(),
                    album_key,
                    disc: number(value(p, "Disc")),
                    track: number(value(p, "Track")),
                });
            }
        }
    }
    Ok(result)
}
