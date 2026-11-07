// SPDX-License-Identifier: GPL-3.0-or-later
//! The files the program keeps: written whole and atomically (a sibling `NAME.new`, then a
//! rename, so a reader never sees a half-written file), stamped by modification time for
//! the watcher's live re-reads, and the on/off switch files (modes, window rules) that
//! share one shape: comment lines, a bare `on`/`off`, and records.
use std::{fs, path::Path, time::SystemTime};

pub(crate) fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// Writes the whole file atomically, creating its directory first.
pub(crate) fn atomic_write(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = path.with_file_name(format!("{name}.new"));
    fs::write(&tmp, bytes)
        .and_then(|_| fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Removes a file; a missing one is already removed.
pub(crate) fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Reads a switch file: blank and `#` lines are skipped, a bare `on`/`off` sets the switch
/// (the last one wins, off by default), every other trimmed line goes to `record`.
pub(crate) fn parse_switch<'a>(text: &'a str, mut record: impl FnMut(&'a str)) -> bool {
    let mut on = false;
    for raw in text.lines() {
        let line = raw.trim();
        match line {
            "" => {}
            _ if line.starts_with('#') => {}
            "on" => on = true,
            "off" => on = false,
            _ => record(line),
        }
    }
    on
}

/// Writes a switch file: the header comment, the switch, then the records.
pub(crate) fn switch_text(
    header: &str,
    on: bool,
    records: impl IntoIterator<Item = String>,
) -> String {
    let mut out = String::from(header);
    out.push_str(if on { "on\n" } else { "off\n" });
    for r in records {
        out.push_str(&r);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_file_round_trip() {
        let text = switch_text("# h\n", true, ["a 1".to_string(), "b\tc".to_string()]);
        assert_eq!(text, "# h\non\na 1\nb\tc\n");
        let mut records = vec![];
        assert!(parse_switch(&text, |r| records.push(r)));
        assert_eq!(records, ["a 1", "b\tc"]);
        assert!(
            !parse_switch("on\n# c\n\noff\nx", |_| {}),
            "the last switch wins"
        );
        assert!(!parse_switch("", |_| panic!("no records")));
    }

    #[test]
    fn atomic_write_names_its_tmp_after_the_file() {
        let dir = std::env::temp_dir().join(format!("g13pad-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let p = dir.join("sub").join("x.bind");
        atomic_write(&p, "hi").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "hi");
        assert!(!dir.join("sub").join("x.bind.new").exists());
        remove(&p).unwrap();
        remove(&p).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
