// SPDX-License-Identifier: GPL-3.0-or-later
//! Profiles by focused window on any X11 window manager that follows EWMH: xfwm4 (XFCE),
//! KWin, Mutter, Marco, Openbox, Fluxbox, awesome, bspwm and the rest. The root window's
//! `_NET_ACTIVE_WINDOW` names the focused window and `_NET_CLIENT_LIST` the managed ones; a
//! `PropertyNotify` on the root reports a change. `focus` uses this when there is no i3.
//!
//! The connection is made by hand rather than through the display library's own lookup:
//! a watcher started by systemd at login has no DISPLAY or XAUTHORITY of its own and takes
//! both from the user manager's environment (see `session`), so the authority file is read
//! here and handed to the connection. Local displays only (`:N`, `:N.S`, `unix:N`).
use crate::focus::Win;
use std::{fs, os::unix::net::UnixStream, path::PathBuf, sync::mpsc};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::{DefaultStream, RustConnection};

const COOKIE: &[u8] = b"MIT-MAGIC-COOKIE-1";
const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

struct X {
    conn: RustConnection,
    root: Window,
    active: Atom,
    clients: Atom,
    desktop: Atom,
    desktop_names: Atom,
    wm_name: Atom,
    utf8: Atom,
    display: String,
    /// The window manager's name, as it announces itself (`Xfwm4`, `Openbox`, `KWin`).
    manager: String,
}

/// The windows the manager lists now, in `_NET_CLIENT_LIST` order (oldest first).
pub fn windows() -> Result<Vec<Win>, String> {
    let x = X::connect()?;
    let names = x.desktop_names();
    let active = x.active_window();
    let mut out = vec![];
    for w in x.cardinals(x.root, x.clients, AtomEnum::WINDOW.into()) {
        if let Some(class) = x.class(w) {
            let desktop = x.cardinals(w, x.desktop, AtomEnum::CARDINAL.into());
            out.push(Win {
                class,
                title: x.title(w),
                workspace: desktop_name(&names, desktop.first().copied()),
                focused: Some(w) == active,
            });
        }
    }
    Ok(out)
}

/// Reports the class of every window that takes focus, until the display goes away
/// (`Err`) or the receiver is gone (`Ok`). Reports what is focused now first.
pub fn follow(tx: &mpsc::Sender<String>) -> Result<(), String> {
    let x = X::connect()?;
    x.conn
        .change_window_attributes(
            x.root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| e.to_string())
        .and_then(|c| c.check().map_err(|e| e.to_string()))
        .map_err(|e| format!("cannot watch the root window: {e}"))?;
    eprintln!(
        "g13map watch: window focus: {} on display {}",
        x.manager, x.display
    );
    if let Some(c) = x.active_window().and_then(|w| x.class(w)) {
        if tx.send(c).is_err() {
            return Ok(());
        }
    }
    loop {
        let ev = x
            .conn
            .wait_for_event()
            .map_err(|e| format!("display lost: {e}"))?;
        if let Event::PropertyNotify(p) = ev {
            if p.window == x.root && p.atom == x.active {
                if let Some(c) = x.active_window().and_then(|w| x.class(w)) {
                    if tx.send(c).is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

impl X {
    fn connect() -> Result<X, String> {
        let display = crate::session::var("DISPLAY")
            .ok_or("no DISPLAY here or in the user manager's environment")?;
        let (path, number, screen) = socket_path(&display)?;
        let authority = crate::session::var("XAUTHORITY")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".Xauthority")));
        let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        let mut auth = authority
            .and_then(|p| fs::read(p).ok())
            .map(|bytes| cookies(&bytes, &number, host.trim().as_bytes()))
            .unwrap_or_default();
        // A file with several entries for the display (a display manager's leftovers) is
        // tried in order, this host's local entry first; no entry at all means no auth.
        auth.push(Default::default());
        let mut conn = None;
        for (name, data) in auth {
            let stream =
                UnixStream::connect(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let (stream, _) = DefaultStream::from_unix_stream(stream)
                .map_err(|e| format!("display {display}: {e}"))?;
            match RustConnection::connect_to_stream_with_auth_info(stream, screen, name, data) {
                Ok(c) => {
                    conn = Some(Ok(c));
                    break;
                }
                Err(e) => conn = Some(Err(format!("display {display}: {e}"))),
            }
        }
        let conn = conn.unwrap()?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| format!("display {display} has no screen {screen}"))?
            .root;
        let atom = |name: &[u8]| -> Result<Atom, String> {
            Ok(conn
                .intern_atom(false, name)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .atom)
        };
        let check = atom(b"_NET_SUPPORTING_WM_CHECK")?;
        let mut x = X {
            root,
            active: atom(b"_NET_ACTIVE_WINDOW")?,
            clients: atom(b"_NET_CLIENT_LIST")?,
            desktop: atom(b"_NET_WM_DESKTOP")?,
            desktop_names: atom(b"_NET_DESKTOP_NAMES")?,
            wm_name: atom(b"_NET_WM_NAME")?,
            utf8: atom(b"UTF8_STRING")?,
            conn,
            display: display.clone(),
            manager: String::new(),
        };
        // The manager announces itself through a window of its own, named after it.
        let Some(announcer) = x
            .cardinals(x.root, check, AtomEnum::WINDOW.into())
            .first()
            .copied()
        else {
            return Err(format!(
                "display {display}: no EWMH window manager (nothing announces _NET_SUPPORTING_WM_CHECK)"
            ));
        };
        x.manager = x.title(announcer);
        if x.manager.is_empty() {
            x.manager = "an unnamed window manager".into();
        }
        Ok(x)
    }

    /// A property's bytes, or nothing: a window gone between two requests is not an error.
    fn bytes(&self, w: Window, prop: Atom, ty: Atom) -> Vec<u8> {
        self.conn
            .get_property(false, w, prop, ty, 0, 1 << 20)
            .ok()
            .and_then(|c| c.reply().ok())
            .filter(|r| r.format != 0)
            .map(|r| r.value)
            .unwrap_or_default()
    }

    fn cardinals(&self, w: Window, prop: Atom, ty: Atom) -> Vec<u32> {
        self.conn
            .get_property(false, w, prop, ty, 0, 1 << 20)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn active_window(&self) -> Option<Window> {
        self.cardinals(self.root, self.active, AtomEnum::WINDOW.into())
            .first()
            .copied()
            .filter(|w| *w != 0)
    }

    fn class(&self, w: Window) -> Option<String> {
        class_of(&self.bytes(w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into()))
    }

    fn title(&self, w: Window) -> String {
        let utf8 = self.bytes(w, self.wm_name, self.utf8);
        let raw = if utf8.is_empty() {
            self.bytes(w, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())
        } else {
            utf8
        };
        String::from_utf8_lossy(&raw).into_owned()
    }

    fn desktop_names(&self) -> Vec<String> {
        strings(&self.bytes(self.root, self.desktop_names, self.utf8))
    }
}

/// `/tmp/.X11-unix/XN` for a local display, with its number and screen.
fn socket_path(display: &str) -> Result<(PathBuf, String, usize), String> {
    let rest = display
        .strip_prefix("unix:")
        .or_else(|| display.strip_prefix(':'))
        .ok_or_else(|| format!("display {display} is not local; only :N displays are followed"))?;
    let (number, screen) = rest.split_once('.').unwrap_or((rest, "0"));
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("cannot parse display {display}"));
    }
    let screen = screen
        .parse()
        .map_err(|_| format!("cannot parse display {display}"))?;
    Ok((
        PathBuf::from(format!("/tmp/.X11-unix/X{number}")),
        number.to_string(),
        screen,
    ))
}

/// An authorization protocol name and its data, as the X setup request carries them.
type Auth = (Vec<u8>, Vec<u8>);
const FAMILY_LOCAL: u16 = 256;
const FAMILY_WILD: u16 = 0xFFFF;

/// The MIT-MAGIC-COOKIE-1 entries for a display number in an Xauthority file, the one a
/// Unix-socket connection wants first: records of big-endian `family u16`, then four
/// length-prefixed strings (address, number, name, data). A display manager leaves
/// several for one display (an Internet-family pair beside the local one, older
/// sessions' cookies), so this host's local entry comes first, then a wildcard, then the
/// rest in file order; the caller tries them in turn. A truncated file ends the list.
fn cookies(bytes: &[u8], number: &str, host: &[u8]) -> Vec<Auth> {
    let mut at = 0;
    let mut field = |n: usize| -> Option<&[u8]> {
        let s = bytes.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let mut found: Vec<(u8, Auth)> = vec![];
    let mut record = || -> Option<()> {
        let family = field(2)?;
        let family = u16::from_be_bytes([family[0], family[1]]);
        let mut strings = vec![];
        for _ in 0..4 {
            let len = field(2)?;
            let len = u16::from_be_bytes([len[0], len[1]]) as usize;
            strings.push(field(len)?.to_vec());
        }
        let [addr, num, name, data] = <[Vec<u8>; 4]>::try_from(strings).ok()?;
        if name == COOKIE && (num.is_empty() || num == number.as_bytes()) {
            let rank = match family {
                FAMILY_LOCAL if addr == host => 0,
                FAMILY_WILD => 1,
                _ => 2,
            };
            found.push((rank, (name, data)));
        }
        Some(())
    };
    while record().is_some() {}
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, c)| c).collect()
}

/// WM_CLASS is `instance\0class\0`: the class, else the instance.
fn class_of(bytes: &[u8]) -> Option<String> {
    let mut parts = strings(bytes).into_iter();
    let instance = parts.next().filter(|s| !s.is_empty());
    parts.next().filter(|s| !s.is_empty()).or(instance)
}

/// NUL-separated strings, a trailing NUL allowed.
fn strings(bytes: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if out.last().is_some_and(|s| s.is_empty()) {
        out.pop();
    }
    out
}

/// A desktop's name from `_NET_DESKTOP_NAMES`, its number from 1 when unnamed, `all` for a
/// window on every desktop, nothing when the manager does not say.
fn desktop_name(names: &[String], index: Option<u32>) -> String {
    match index {
        None => String::new(),
        Some(ALL_DESKTOPS) => "all".into(),
        Some(i) => names
            .get(i as usize)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| (i + 1).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_displays_map_to_their_socket() {
        let (p, n, s) = socket_path(":0").unwrap();
        assert_eq!(
            (p.to_str().unwrap(), n.as_str(), s),
            ("/tmp/.X11-unix/X0", "0", 0)
        );
        let (p, n, s) = socket_path(":99.1").unwrap();
        assert_eq!(
            (p.to_str().unwrap(), n.as_str(), s),
            ("/tmp/.X11-unix/X99", "99", 1)
        );
        assert_eq!(socket_path("unix:3").unwrap().1, "3");
        for bad in ["", ":", ":x", "host:0", "localhost:10.0", ":0.z"] {
            assert!(socket_path(bad).is_err(), "{bad}");
        }
    }

    fn record(family: u16, addr: &[u8], num: &[u8], name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut v = family.to_be_bytes().to_vec();
        for s in [addr, num, name, data] {
            v.extend((s.len() as u16).to_be_bytes());
            v.extend(s);
        }
        v
    }

    #[test]
    fn the_cookies_for_the_display_come_local_first() {
        // A display manager's file: an Internet-family pair for :0 (stale), the local one
        // last, another display, a wildcard, a foreign protocol.
        let mut file = record(0, b"\x7f\0\0\x01", b"0", COOKIE, b"inet");
        file.extend(record(
            6,
            b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01",
            b"0",
            COOKIE,
            b"inet6",
        ));
        file.extend(record(256, b"host", b"1", COOKIE, b"one"));
        file.extend(record(256, b"host", b"0", b"OTHER-PROTO", b"no"));
        file.extend(record(256, b"host", b"0", COOKIE, b"zero"));
        file.extend(record(256, b"other", b"0", COOKIE, b"elsewhere"));
        file.extend(record(0xFFFF, b"", b"", COOKIE, b"wild"));
        let data = |n: &str, h: &[u8]| -> Vec<Vec<u8>> {
            cookies(&file, n, h).into_iter().map(|(_, d)| d).collect()
        };
        assert_eq!(
            data("0", b"host"),
            [&b"zero"[..], b"wild", b"inet", b"inet6", b"elsewhere"]
        );
        assert_eq!(data("1", b"host"), [&b"one"[..], b"wild"]);
        assert_eq!(data("7", b"host"), [&b"wild"[..]]);
        // On another host the local entry is just one of the rest.
        assert_eq!(data("0", b"zzz")[0], b"wild");
        // A truncated file keeps what was whole before the cut.
        assert_eq!(cookies(&file[..file.len() - 2], "7", b"host"), vec![]);
        assert_eq!(cookies(&file[..file.len() - 2], "1", b"host").len(), 1);
        assert!(cookies(b"", "0", b"host").is_empty());
        assert!(cookies(&record(256, b"h", b"0", b"OTHER-PROTO", b"x"), "0", b"h").is_empty());
    }

    #[test]
    fn classes_titles_and_desktops_decode() {
        assert_eq!(
            class_of(b"xfce4-terminal\0Xfce4-terminal\0").as_deref(),
            Some("Xfce4-terminal")
        );
        assert_eq!(class_of(b"xeyes\0\0").as_deref(), Some("xeyes"));
        assert_eq!(class_of(b"xeyes").as_deref(), Some("xeyes"));
        assert_eq!(class_of(b""), None);
        assert_eq!(class_of(b"\0\0"), None);
        let names = strings(b"Workspace 1\0Workspace 2\0\0");
        assert_eq!(names, vec!["Workspace 1", "Workspace 2", ""]);
        assert_eq!(desktop_name(&names, Some(1)), "Workspace 2");
        assert_eq!(desktop_name(&names, Some(2)), "3");
        assert_eq!(desktop_name(&names, Some(9)), "10");
        assert_eq!(desktop_name(&names, Some(ALL_DESKTOPS)), "all");
        assert_eq!(desktop_name(&names, None), "");
    }
}
