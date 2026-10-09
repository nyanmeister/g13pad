// SPDX-License-Identifier: GPL-3.0-or-later
//! Profiles by focused window. The focused window's WM_CLASS class (`firefox`, `kitty`,
//! `steam`) looks up a profile in `~/.config/g13map/focus`, and `g13map watch` switches to
//! it. Lit M-keys override the window's profile while modes are on; MR clears back to it
//! (see `modes`).
//!
//! Two sources, the first that answers wins: i3 (and sway, which speaks the same IPC)
//! reports focus changes over its socket; any other X11 window manager is read through the
//! EWMH root-window properties (`xfocus`). The i3 IPC is spoken directly (i3 4.x): the magic
//! "i3-ipc", a u32 length, a u32 type and the JSON payload, both ways; an event reply has
//! the top bit of its type set.
use serde_json::Value;
use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    thread,
    time::Duration,
};

/// The editor's own window: focusing it keeps whatever profile is shown.
pub const EDITOR_CLASS: &str = "g13map";
const MAGIC: &[u8; 6] = b"i3-ipc";
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;
const EVENT_WINDOW: u32 = 0x8000_0003;
const MAX_PAYLOAD: usize = 32 * 1024 * 1024;

pub fn path() -> PathBuf {
    crate::config_dir().join("focus")
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Rules {
    pub on: bool,
    /// (WM_CLASS class, profile), in file order.
    pub rules: Vec<(String, String)>,
}

impl Rules {
    pub fn load() -> Rules {
        Rules::parse(&fs::read_to_string(path()).unwrap_or_default())
    }
    /// `on`/`off`, then `CLASS<TAB>PROFILE` lines (or the last run of spaces as the split).
    pub fn parse(text: &str) -> Rules {
        let mut r = Rules::default();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match line {
                "on" => r.on = true,
                "off" => r.on = false,
                _ => {
                    let split = line
                        .split_once('\t')
                        .or_else(|| line.rsplit_once(' '))
                        .map(|(c, p)| (c.trim(), p.trim()));
                    if let Some((c, p)) = split.filter(|(c, p)| !c.is_empty() && !p.is_empty()) {
                        r.set(c, Some(p));
                    }
                }
            }
        }
        r
    }
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "# g13map: profiles by focused window (i3, or any X11 window manager).\n\
             # `CLASS<TAB>PROFILE`, CLASS being the\n\
             # window's WM_CLASS class as the Windows… list shows it.\n",
        );
        out.push_str(if self.on { "on\n" } else { "off\n" });
        for (c, p) in &self.rules {
            out.push_str(&format!("{c}\t{p}\n"));
        }
        out
    }
    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all(crate::config_dir()).map_err(|e| e.to_string())?;
        let p = path();
        let tmp = p.with_extension("new");
        fs::write(&tmp, self.to_text())
            .and_then(|_| fs::rename(&tmp, &p))
            .map_err(|e| format!("{}: {e}", p.display()))
    }
    pub fn profile_for(&self, class: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|(c, _)| c == class)
            .map(|(_, p)| p.as_str())
    }
    /// Sets or (with None) removes the rule for a class.
    pub fn set(&mut self, class: &str, profile: Option<&str>) {
        self.rules.retain(|(c, _)| c != class);
        if let Some(p) = profile {
            self.rules.push((class.to_string(), p.to_string()));
        }
    }
}

/// A window the manager lists, as the Windows… list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub class: String,
    pub title: String,
    pub workspace: String,
    pub focused: bool,
}

/// The windows the manager lists now: i3's tree in order, else the X11 client list.
pub fn windows() -> Result<Vec<Win>, String> {
    match socket() {
        Ok(s) => i3_windows(s),
        Err(i3) => crate::xfocus::windows().map_err(|x| format!("no i3 ({i3}); {x}")),
    }
}

fn i3_windows(mut s: UnixStream) -> Result<Vec<Win>, String> {
    let timeout = Some(Duration::from_secs(2));
    s.set_read_timeout(timeout).map_err(|e| e.to_string())?;
    s.set_write_timeout(timeout).map_err(|e| e.to_string())?;
    write_msg(&mut s, GET_TREE, b"").map_err(|e| format!("i3 ipc: {e}"))?;
    let (_, payload) = read_msg(&mut s).map_err(|e| format!("i3 ipc: {e}"))?;
    let tree: Value = serde_json::from_slice(&payload).map_err(|e| format!("i3 tree: {e}"))?;
    let mut out = vec![];
    walk(&tree, "", &mut out);
    Ok(out)
}

/// The class of the focused window, if any.
pub fn focused_class() -> Option<String> {
    windows()
        .ok()?
        .into_iter()
        .find(|w| w.focused)
        .map(|w| w.class)
}

/// Runs for the life of the process: reports the class of every window that takes focus.
/// Reconnects every few seconds if no manager is there or it goes away; the reason is
/// logged when it changes, not every try.
pub fn follow(tx: mpsc::Sender<String>) {
    let mut said = String::new();
    loop {
        // Once connected, report what is focused now: at login the watcher started before
        // the desktop, and nothing may change focus for a while.
        let connected = match socket() {
            Ok(s) => {
                eprintln!("g13map watch: window focus: i3");
                if let Some(c) = i3_windows(s).ok().and_then(focused) {
                    let _ = tx.send(c);
                }
                socket().and_then(|s| subscribe(s, &tx))
            }
            Err(i3) => crate::xfocus::follow(&tx).map_err(|x| format!("no i3 ({i3}); {x}")),
        };
        match connected {
            Ok(()) => return, // the receiver is gone
            Err(e) => {
                if e != said {
                    eprintln!("g13map watch: window focus: {e}; retrying every 5 s");
                    said = e;
                }
            }
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn focused(wins: Vec<Win>) -> Option<String> {
    wins.into_iter().find(|w| w.focused).map(|w| w.class)
}

fn subscribe(mut s: UnixStream, tx: &mpsc::Sender<String>) -> Result<(), String> {
    write_msg(&mut s, SUBSCRIBE, br#"["window"]"#).map_err(|e| e.to_string())?;
    loop {
        let (ty, payload) = read_msg(&mut s).map_err(|e| e.to_string())?;
        if ty != EVENT_WINDOW {
            continue;
        }
        let ev: Value = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
        if ev["change"].as_str() == Some("focus") {
            if let Some(c) = class_of(&ev["container"]) {
                if tx.send(c).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

/// Connects to i3's IPC socket, trying each place it can be known from (`candidates`).
fn socket() -> Result<UnixStream, String> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let paths = candidates(runtime.as_deref(), crate::session::var);
    let mut last = None;
    for path in &paths {
        match UnixStream::connect(path) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(format!("{}: {e}", path.display())),
        }
    }
    Err(last.unwrap_or_else(|| "i3 socket not found (is i3 running?)".into()))
}

/// Where i3's socket may be, most authoritative first: `I3SOCK` (or sway's `SWAYSOCK`) as it is set for its
/// children; `i3 --get-socketpath`, which reads the root window property, given a display
/// (the process's own or the user manager's, see `session`); `I3SOCK` in the user manager's
/// environment; and i3 4.21's own `$XDG_RUNTIME_DIR/i3/ipc-socket.PID`, newest first, which
/// needs no display at all. A watcher started by systemd at login has none of the first three.
fn candidates(runtime: Option<&Path>, var: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    let mut add = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    for key in ["I3SOCK", "SWAYSOCK"] {
        if let Some(p) = env::var_os(key) {
            add(PathBuf::from(p));
        }
    }
    if let Some(display) = var("DISPLAY") {
        let mut c = Command::new("i3");
        c.arg("--get-socketpath").env("DISPLAY", display);
        if let Some(auth) = var("XAUTHORITY") {
            c.env("XAUTHORITY", auth);
        }
        if let Ok(o) = c.output() {
            if o.status.success() {
                let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !p.is_empty() {
                    add(PathBuf::from(p));
                }
            }
        }
    }
    for key in ["I3SOCK", "SWAYSOCK"] {
        if let Some(p) = var(key) {
            add(PathBuf::from(p));
        }
    }
    if let Some(dir) = runtime {
        let mut found: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(dir.join("i3"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("ipc-socket."))
            })
            .filter_map(|e| {
                let t = e.metadata().ok()?.modified().ok()?;
                Some((t, e.path()))
            })
            .collect();
        found.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, p) in found {
            add(p);
        }
    }
    out
}

fn write_msg<W: Write>(w: &mut W, ty: u32, payload: &[u8]) -> io::Result<()> {
    w.write_all(MAGIC)?;
    w.write_all(&(payload.len() as u32).to_ne_bytes())?;
    w.write_all(&ty.to_ne_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

fn read_msg<R: Read>(r: &mut R) -> io::Result<(u32, Vec<u8>)> {
    let mut head = [0u8; 14];
    r.read_exact(&mut head)?;
    if &head[..6] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an i3-ipc message",
        ));
    }
    let len = u32::from_ne_bytes(head[6..10].try_into().unwrap()) as usize;
    if len > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "i3 IPC payload exceeds 32 MiB",
        ));
    }
    let ty = u32::from_ne_bytes(head[10..14].try_into().unwrap());
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok((ty, payload))
}

/// WM_CLASS class, else its instance (i3 gives both under `window_properties`).
fn class_of(container: &Value) -> Option<String> {
    let props = &container["window_properties"];
    props["class"]
        .as_str()
        .or_else(|| props["instance"].as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn walk(node: &Value, workspace: &str, out: &mut Vec<Win>) {
    let ws = if node["type"].as_str() == Some("workspace") {
        node["name"].as_str().unwrap_or(workspace)
    } else {
        workspace
    };
    if !node["window"].is_null() {
        if let Some(class) = class_of(node) {
            out.push(Win {
                class,
                title: node["name"].as_str().unwrap_or("").to_string(),
                workspace: ws.to_string(),
                focused: node["focused"].as_bool().unwrap_or(false),
            });
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(kids) = node[key].as_array() {
            for k in kids {
                walk(k, ws, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rules_round_trip() {
        let r = Rules::parse("# c\non\nfirefox\tbrowsing\nkitty   shell\nbad\n\tnope\n");
        assert!(r.on);
        assert_eq!(r.profile_for("firefox"), Some("browsing"));
        assert_eq!(r.profile_for("kitty"), Some("shell"));
        assert_eq!(r.profile_for("bad"), None);
        assert_eq!(r.rules.len(), 2);
        assert_eq!(Rules::parse(&r.to_text()), r);
        let mut r2 = r.clone();
        r2.set("firefox", Some("other"));
        r2.set("kitty", None);
        assert_eq!(r2.rules, vec![("firefox".to_string(), "other".to_string())]);
        assert!(!Rules::parse("").on);
    }

    #[test]
    fn ipc_framing_round_trips() {
        let mut buf = vec![];
        write_msg(&mut buf, SUBSCRIBE, br#"["window"]"#).unwrap();
        assert_eq!(&buf[..6], MAGIC);
        let (ty, payload) = read_msg(&mut Cursor::new(buf)).unwrap();
        assert_eq!((ty, payload.as_slice()), (SUBSCRIBE, &br#"["window"]"#[..]));
        let mut huge = Vec::from(&MAGIC[..]);
        huge.extend_from_slice(&u32::MAX.to_ne_bytes());
        huge.extend_from_slice(&GET_TREE.to_ne_bytes());
        assert_eq!(
            read_msg(&mut Cursor::new(huge)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(read_msg(&mut Cursor::new(b"nonsense-header")).is_err());
    }

    #[test]
    fn socket_candidates_come_from_the_manager_and_the_runtime_dir() {
        let dir = env::temp_dir().join(format!("g13map-i3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("i3")).unwrap();
        let old = dir.join("i3/ipc-socket.100");
        let new = dir.join("i3/ipc-socket.200");
        fs::write(&old, "").unwrap();
        thread::sleep(Duration::from_millis(20));
        fs::write(&new, "").unwrap();
        fs::write(dir.join("i3/other"), "").unwrap();
        // A bare process (no I3SOCK, no display) still finds i3's own socket, newest first.
        let bare = candidates(Some(&dir), |_| None);
        let in_dir: Vec<_> = bare.iter().filter(|p| p.starts_with(&dir)).collect();
        assert_eq!(in_dir, vec![&new, &old]);
        // The user manager's I3SOCK comes before the runtime-directory guesses; a display
        // from the manager is consulted through `i3 --get-socketpath` (not run here).
        let manager = candidates(Some(&dir), |k| {
            (k == "I3SOCK").then(|| "/run/user/1/i3/ipc-socket.7".to_string())
        });
        let pos = |p: &Path| manager.iter().position(|c| c == p).unwrap();
        assert!(pos(Path::new("/run/user/1/i3/ipc-socket.7")) < pos(&new));
        assert!(candidates(None, |_| None)
            .iter()
            .all(|p| !p.starts_with(&dir)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tree_walk_finds_windows_and_workspaces() {
        let tree: Value = serde_json::from_str(
            r#"{"type":"root","nodes":[{"type":"output","name":"DP-1","nodes":[
                {"type":"workspace","name":"2","nodes":[
                    {"type":"con","window":1,"name":"urxvt","focused":false,
                     "window_properties":{"class":"URxvt","instance":"urxvt"}},
                    {"type":"con","window":2,"name":"G13","focused":true,
                     "window_properties":{"class":"g13map","instance":""}}],
                 "floating_nodes":[{"type":"floating_con","nodes":[
                    {"type":"con","window":3,"name":"eyes","focused":false,
                     "window_properties":{"instance":"xeyes"}}]}]},
                {"type":"workspace","name":"3","nodes":[{"type":"con","window":null,"nodes":[]}]}]}]}"#,
        )
        .unwrap();
        let mut out = vec![];
        walk(&tree, "", &mut out);
        let got: Vec<(String, String, bool)> = out
            .iter()
            .map(|w| (w.class.clone(), w.workspace.clone(), w.focused))
            .collect();
        assert_eq!(
            got,
            vec![
                ("URxvt".into(), "2".into(), false),
                ("g13map".into(), "2".into(), true),
                ("xeyes".into(), "2".into(), false),
            ]
        );
        let ev: Value = serde_json::from_str(
            r#"{"change":"focus","container":{"window_properties":{"class":"floorp"}}}"#,
        )
        .unwrap();
        assert_eq!(class_of(&ev["container"]).as_deref(), Some("floorp"));
    }
}
