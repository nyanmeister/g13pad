// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded local action client; collection lives in the independent daemon.
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Stop,
    Next,
    Previous,
    NextAlbum,
    PreviousAlbum,
    RateUp,
    RateDown,
    Favourite,
    Bookmark,
    Resume,
    Random,
    ShuffleAlbums,
    SmartShuffle,
}
impl Action {
    pub const ALL: [Self; 14] = [
        Self::Toggle,
        Self::Stop,
        Self::Next,
        Self::Previous,
        Self::NextAlbum,
        Self::PreviousAlbum,
        Self::RateUp,
        Self::RateDown,
        Self::Favourite,
        Self::Bookmark,
        Self::Resume,
        Self::Random,
        Self::ShuffleAlbums,
        Self::SmartShuffle,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Stop => "stop",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::NextAlbum => "next-album",
            Self::PreviousAlbum => "previous-album",
            Self::RateUp => "rate-up",
            Self::RateDown => "rate-down",
            Self::Favourite => "favourite",
            Self::Bookmark => "bookmark",
            Self::Resume => "resume",
            Self::Random => "random",
            Self::ShuffleAlbums => "shuffle-albums",
            Self::SmartShuffle => "smart-shuffle",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Play / pause",
            Self::Stop => "Stop playback",
            Self::Next => "Next track",
            Self::Previous => "Previous track",
            Self::NextAlbum => "Next album",
            Self::PreviousAlbum => "Previous album",
            Self::RateUp => "Rating +1 star",
            Self::RateDown => "Rating -1 star",
            Self::Favourite => "Toggle favourite",
            Self::Bookmark => "Bookmark position",
            Self::Resume => "Resume bookmark",
            Self::Random => "Toggle MPD random",
            Self::ShuffleAlbums => "Shuffle queued albums",
            Self::SmartShuffle => "Smart shuffle queued albums",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == s)
    }
    pub fn binding(self) -> String {
        format!(">MPD:{};", self.key())
    }
    pub fn from_binding(s: &str) -> Option<Self> {
        Self::parse(s.strip_prefix(">MPD:")?.strip_suffix(';')?)
    }
}
pub fn runtime_dir() -> Result<PathBuf, String> {
    std::env::var_os("G13MUSIC_RUNTIME_DIR")
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(|p| PathBuf::from(p).join("g13music").into_os_string())
        })
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is required".into())
}
pub fn request(command: &str) -> Result<String, String> {
    request_timeout(command, Duration::from_secs(15))
}
pub fn request_timeout(command: &str, timeout: Duration) -> Result<String, String> {
    if Action::parse(command).is_none() && command != "status" && command != "track" {
        return Err("unknown music action".into());
    }
    let mut socket = UnixStream::connect(runtime_dir()?.join("control.sock"))
        .map_err(|e| format!("g13music service unavailable: {e}"))?;
    let timeout = Some(timeout);
    socket
        .set_read_timeout(timeout)
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(timeout)
        .map_err(|e| e.to_string())?;
    writeln!(socket, "{command}").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    socket
        .take(8193)
        .read_to_string(&mut reply)
        .map_err(|e| e.to_string())?;
    if reply.len() > 8192 {
        return Err("music response exceeded limit".into());
    }
    if let Some(s) = reply.strip_prefix("OK ") {
        Ok(s.trim_end().into())
    } else {
        Err(reply
            .trim_end()
            .strip_prefix("ERR ")
            .unwrap_or(&reply)
            .into())
    }
}

/// Public, read-only endpoint identity shared by the collector and LCD client.
/// libmpdclient strips credentials; they never enter the snapshot or logs.
pub fn daemon_endpoint() -> Result<String, String> {
    use std::{
        ffi::{c_char, c_uint, c_void, CStr},
        ptr,
    };
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
    }
    unsafe {
        let p = mpd_settings_new(
            if std::env::var_os("MPD_HOST").is_some() {
                ptr::null()
            } else {
                c"127.0.0.1".as_ptr()
            },
            0,
            300,
            ptr::null(),
            ptr::null(),
        );
        if p.is_null() {
            return Err("MPD settings unavailable".into());
        }
        let raw = mpd_settings_get_host(p);
        let result = if raw.is_null() {
            Err("MPD address unavailable".into())
        } else {
            CStr::from_ptr(raw)
                .to_str()
                .map_err(|_| "invalid MPD address".into())
                .map(|host| {
                    let host = if host == "localhost" {
                        "127.0.0.1"
                    } else {
                        host
                    };
                    if host.starts_with('/') || host.starts_with('@') {
                        format!("unix:{host}")
                    } else {
                        format!("tcp:{host}:{}", mpd_settings_get_port(p))
                    }
                })
        };
        mpd_settings_free(p);
        result
    }
}
pub fn local_date(timestamp: u64) -> Option<String> {
    let timestamp: libc::time_t = timestamp.try_into().ok()?;
    if timestamp <= 0 {
        return None;
    }
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe { libc::localtime_r(&timestamp, tm.as_mut_ptr()) }.is_null() {
        return None;
    }
    let tm = unsafe { tm.assume_init() };
    let year = tm.tm_year.checked_add(1900)?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some(format!("{year:04}-{:02}-{:02}", tm.tm_mon + 1, tm.tm_mday))
}

/// Linux Unix-socket peer identity. TCP does not provide a useful process ID.
pub fn peer_process_id(fd: i32) -> Option<u32> {
    let mut cred = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            cred.as_mut_ptr().cast(),
            &mut size,
        )
    } != 0
        || size as usize != std::mem::size_of::<libc::ucred>()
    {
        return None;
    }
    let cred = unsafe { cred.assume_init() };
    (cred.pid > 0).then_some(cred.pid as u32)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_action_tokens() {
        for a in Action::ALL {
            assert_eq!(Action::from_binding(&a.binding()), Some(a));
        }
        for s in [
            ">MPD:next;trailing",
            ">MPD:next",
            ">MPD:next;\n",
            ">MPD:unknown;",
        ] {
            assert_eq!(Action::from_binding(s), None);
        }
    }
}
