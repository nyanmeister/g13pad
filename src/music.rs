// SPDX-License-Identifier: GPL-3.0-or-later
//! Profile-independent daemon controls, dispatched away from the LCD/UI thread.
pub use g13pad_music::Action;
use std::sync::mpsc::{self, SyncSender};
pub struct Worker {
    tx: SyncSender<Action>,
}
impl Worker {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::sync_channel::<Action>(8);
        std::thread::spawn(move || {
            while let Ok(a) = rx.recv() {
                if let Err(e) = g13pad_music::request(a.key()) {
                    eprintln!("music action: {e}");
                    let _ = crate::overlay::notify(&e);
                }
            }
        });
        Self { tx }
    }
    pub fn action(&self, action: Action) {
        if self.tx.try_send(action).is_err() {
            eprintln!("music action queue full; action ignored");
        }
    }
}
