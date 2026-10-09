// SPDX-License-Identifier: GPL-3.0-or-later
//! Profiles by focused window on a Wayland compositor that offers
//! `wlr-foreign-toplevel-management` (sway, labwc, river, wayfire, Hyprland and the rest of
//! the wlroots family): the compositor lists every toplevel with its app id, title and
//! state, and says `done` after each batch of changes. The app id stands in for the X11
//! class in the rules (`firefox`, `foot`, `org.kde.konsole`); an Xwayland window's app id is
//! its WM_CLASS class, so a game under Proton keeps the class it has on X11.
//!
//! The socket is `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`, the display name from this process or
//! the user manager (see `session`), else the newest `wayland-N` in the runtime directory:
//! a watcher started by systemd at login has no WAYLAND_DISPLAY of its own.
use crate::focus::Win;
use std::{
    collections::HashMap,
    env, fs,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::mpsc,
};
use wayland_client::{
    backend::ObjectId,
    protocol::{wl_registry, wl_registry::WlRegistry},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self as handle, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self as manager, ZwlrForeignToplevelManagerV1},
};

const INTERFACE: &str = "zwlr_foreign_toplevel_manager_v1";
const ACTIVATED: u32 = handle::State::Activated as u32;

#[derive(Default)]
struct Top {
    app_id: String,
    title: String,
    activated: bool,
    /// What the compositor has said since the last `done`.
    pending: (Option<String>, Option<String>, Option<bool>),
}

#[derive(Default)]
struct State {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    /// Toplevels in the order the compositor announced them (oldest first).
    order: Vec<ObjectId>,
    tops: HashMap<ObjectId, Top>,
    /// The compositor ended the manager (it is going away).
    finished: bool,
}

struct W {
    conn: Connection,
    queue: EventQueue<State>,
    state: State,
    display: String,
}

/// The toplevels the compositor lists now, oldest first.
pub fn windows() -> Result<Vec<Win>, String> {
    let mut w = W::connect()?;
    w.roundtrip()?;
    w.roundtrip()?;
    Ok(w.state
        .order
        .iter()
        .filter_map(|id| w.state.tops.get(id))
        .filter(|t| !t.app_id.is_empty())
        .map(|t| Win {
            class: t.app_id.clone(),
            title: t.title.clone(),
            workspace: String::new(),
            focused: t.activated,
        })
        .collect())
}

/// Reports the app id of every toplevel that becomes active, until the compositor goes
/// away (`Err`) or the receiver is gone (`Ok`). Reports what is active now first.
pub fn follow(tx: &mpsc::Sender<String>) -> Result<(), String> {
    let mut w = W::connect()?;
    w.roundtrip()?;
    w.roundtrip()?;
    eprintln!(
        "g13map watch: window focus: {} on Wayland display {}",
        compositor(),
        w.display
    );
    let mut last = None;
    loop {
        if let Some(active) = w.state.active() {
            if last.as_deref() != Some(active.as_str()) {
                if tx.send(active.clone()).is_err() {
                    return Ok(());
                }
                last = Some(active);
            }
        }
        if w.state.finished {
            return Err(format!(
                "display {}: the compositor withdrew the toplevel list",
                w.display
            ));
        }
        w.queue
            .blocking_dispatch(&mut w.state)
            .map_err(|e| format!("display {}: {e}", w.display))?;
    }
}

/// The compositor as the session names it (`sway`, `labwc`), else a generic name: the
/// protocol does not say.
fn compositor() -> String {
    crate::session::var("XDG_CURRENT_DESKTOP")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "a Wayland compositor".into())
}

impl W {
    fn connect() -> Result<W, String> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or("no XDG_RUNTIME_DIR")?;
        let (path, display) = socket(&runtime, crate::session::var("WAYLAND_DISPLAY"))
            .ok_or("no WAYLAND_DISPLAY here or in the user manager's environment, and no wayland-N socket in the runtime directory")?;
        let stream = UnixStream::connect(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let conn = Connection::from_socket(stream)
            .map_err(|e| format!("Wayland display {display}: {e}"))?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut state = State::default();
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("Wayland display {display}: {e}"))?;
        if state.manager.is_none() {
            return Err(format!(
                "Wayland display {display}: the compositor offers no {INTERFACE} (X11 windows under \
                 Xwayland may still be followed through the X11 display)"
            ));
        }
        Ok(W {
            conn,
            queue,
            state,
            display,
        })
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|e| format!("Wayland display {}: {e}", self.display))?;
        self.conn.flush().map_err(|e| e.to_string())
    }
}

impl State {
    fn active(&self) -> Option<String> {
        self.order
            .iter()
            .filter_map(|id| self.tops.get(id))
            .find(|t| t.activated && !t.app_id.is_empty())
            .map(|t| t.app_id.clone())
    }
}

/// The socket for a display name, else the newest `wayland-N` in the runtime directory.
fn socket(runtime: &Path, display: Option<String>) -> Option<(PathBuf, String)> {
    if let Some(d) = display.filter(|d| !d.is_empty()) {
        let p = if d.starts_with('/') {
            PathBuf::from(&d)
        } else {
            runtime.join(&d)
        };
        return Some((p, d));
    }
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(runtime)
        .ok()?
        .flatten()
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.strip_prefix("wayland-").is_some_and(|rest| {
                    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
                })
            })
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    found.sort_by_key(|a| std::cmp::Reverse(a.0));
    let p = found.into_iter().next()?.1;
    let name = p.file_name()?.to_str()?.to_string();
    Some((p, name))
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        s: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == INTERFACE && s.manager.is_none() {
                s.manager = Some(registry.bind(name, version.min(3), qh, ()));
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            manager::Event::Toplevel { toplevel } => {
                s.order.push(toplevel.id());
                s.tops.insert(toplevel.id(), Top::default());
            }
            manager::Event::Finished => s.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        manager::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        s: &mut Self,
        h: &ZwlrForeignToplevelHandleV1,
        event: handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(t) = s.tops.get_mut(&h.id()) else {
            return;
        };
        match event {
            handle::Event::AppId { app_id } => t.pending.1 = Some(app_id),
            handle::Event::Title { title } => t.pending.0 = Some(title),
            handle::Event::State { state } => t.pending.2 = Some(activated(&state)),
            handle::Event::Done => {
                let (title, app_id, act) = std::mem::take(&mut t.pending);
                if let Some(v) = title {
                    t.title = v;
                }
                if let Some(v) = app_id {
                    t.app_id = v;
                }
                if let Some(v) = act {
                    t.activated = v;
                }
            }
            handle::Event::Closed => {
                s.tops.remove(&h.id());
                s.order.retain(|id| *id != h.id());
                h.destroy();
            }
            _ => {}
        }
    }
}

/// The `state` array is native-endian u32 entries; one of them may be `activated`.
fn activated(bytes: &[u8]) -> bool {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .any(|c| u32::from_ne_bytes(*c) == ACTIVATED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_arrays_decode() {
        let mut v = vec![];
        for n in [0u32, 2] {
            v.extend(n.to_ne_bytes());
        }
        assert!(activated(&v));
        assert!(!activated(&0u32.to_ne_bytes()));
        assert!(!activated(&[]));
        assert!(!activated(&[2, 0, 0])); // a short tail is ignored
    }

    #[test]
    fn the_newest_socket_is_found_when_the_display_is_unnamed() {
        let dir = env::temp_dir().join(format!("g13map-wl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("wayland-0"), "").unwrap();
        fs::write(dir.join("wayland-0.lock"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("wayland-1"), "").unwrap();
        fs::write(dir.join("wayland-x"), "").unwrap();
        assert_eq!(socket(&dir, None).unwrap().1, "wayland-1");
        assert_eq!(
            socket(&dir, Some("wayland-7".into())).unwrap(),
            (dir.join("wayland-7"), "wayland-7".to_string())
        );
        assert_eq!(
            socket(&dir, Some("/tmp/sock".into())).unwrap().0,
            Path::new("/tmp/sock")
        );
        assert_eq!(socket(&dir, Some(String::new())).unwrap().1, "wayland-1");
        fs::remove_dir_all(&dir).unwrap();
        assert!(socket(&dir, None).is_none());
    }
}
