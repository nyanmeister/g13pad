// SPDX-License-Identifier: GPL-3.0-or-later
//! Profile-selected System pressure display. The editor always has LCD priority.
pub mod data;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// The computer-load screen: /proc/pressure sampled once a second.
pub fn start(rest: [u8; 3]) -> crate::screen::Live {
    start_from(rest, PathBuf::from("/proc/pressure"))
}

fn start_from(rest: [u8; 3], root: PathBuf) -> crate::screen::Live {
    let mut sampler = data::Sampler::new(root, Instant::now());
    crate::screen::Live::start(rest, Duration::from_secs(1), "system pressure", move |_| {
        sampler.sample(Instant::now());
        (crate::lcd::Bitmap(sampler.frame().to_vec()), None)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_can_take_the_lock_and_pressure_resumes_after_it_closes() {
        let sandbox = crate::test_support::Sandbox::new("pressure-editor");
        let pipe = crate::test_support::PanelPipe::new();
        let root = sandbox.dir.join("psi");
        std::fs::create_dir_all(&root).unwrap();
        for name in ["cpu", "memory", "io"] {
            std::fs::write(root.join(name), "some avg10=0.00 total=100\n").unwrap();
        }
        let live = start_from([0, 0, 255], root);
        assert_eq!(pipe.read().len(), crate::lcd::BYTES);
        let path = crate::daemon::runtime_file("editor").unwrap();
        let editor = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        editor
            .try_lock()
            .expect("System pressure blocked editor opening");
        std::thread::sleep(Duration::from_millis(100));
        pipe.expect_quiet();
        drop(editor);
        assert_eq!(pipe.read().len(), crate::lcd::BYTES);
        drop(live);
        pipe.expect_quiet();
    }
}
