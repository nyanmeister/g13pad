// SPDX-License-Identifier: GPL-3.0-or-later
//! Background Codex command sessions: group the harness's thread/tool-tagged
//! processes, then remember only their outer PID and start time. The display
//! checks those PIDs so a finished command disappears without another hook.
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Process {
    pid: u32,
    parent: u32,
    start: u64,
}

fn stat(root: &Path, pid: u32) -> Option<Process> {
    let text = fs::read_to_string(root.join(pid.to_string()).join("stat")).ok()?;
    let (_, tail) = text.rsplit_once(')')?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    if matches!(*fields.first()?, "Z" | "X") {
        return None;
    }
    Some(Process {
        pid,
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
    })
}

fn roots(root: &Path, thread: &str, observer: u32) -> Vec<Process> {
    let mut excluded = HashSet::new();
    let mut pid = observer;
    while pid > 0 && excluded.len() < 64 && excluded.insert(pid) {
        let Some(p) = stat(root, pid) else { break };
        pid = p.parent;
    }
    let mut groups: HashMap<Vec<u8>, Vec<Process>> = HashMap::new();
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let thread = format!("CODEX_THREAD_ID={thread}");
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if excluded.contains(&pid) {
            continue;
        }
        let Ok(file) = fs::File::open(entry.path().join("environ")) else {
            continue;
        };
        let mut env = Vec::new();
        if file.take(65536).read_to_end(&mut env).is_err() {
            continue;
        }
        if !env.split(|b| *b == 0).any(|v| v == thread.as_bytes()) {
            continue;
        }
        let Some(call) = env
            .split(|b| *b == 0)
            .find_map(|v| v.strip_prefix(b"CODEX_TOOL_CALL_ID="))
        else {
            continue;
        };
        if call.is_empty() || call.len() > 256 {
            continue;
        }
        if let Some(p) = stat(root, pid) {
            groups.entry(call.to_vec()).or_default().push(p);
        }
    }
    let mut out = Vec::new();
    for group in groups.values() {
        let pids: HashSet<_> = group.iter().map(|p| p.pid).collect();
        // Sandbox, shell and its children are one tool session. Its outer process
        // survives while the command is running, even if the shell execs a program.
        out.extend(group.iter().filter(|p| !pids.contains(&p.parent)).copied());
    }
    out.sort_by_key(|p| p.pid);
    out.truncate(super::MAX_AGENTS as usize);
    out
}

fn path(win: u64) -> Option<PathBuf> {
    Some(super::feed::dir().ok()?.join(format!("shells-{win}")))
}

pub(super) fn refresh(win: u64, thread: &str) {
    if thread.is_empty() {
        return;
    }
    let Some(path) = path(win) else { return };
    let processes = roots(Path::new("/proc"), thread, std::process::id());
    let text: String = processes
        .iter()
        .map(|p| format!("{} {}\n", p.pid, p.start))
        .collect();
    let tmp = path.with_extension("new");
    let _ = fs::write(&tmp, text).and_then(|_| fs::rename(tmp, path));
}

pub(super) fn count(win: u64) -> Option<u32> {
    let text = fs::read_to_string(path(win)?).ok()?;
    Some(
        text.lines()
            .filter(|line| {
                let mut fields = line.split_whitespace();
                let (Some(pid), Some(start)) = (
                    fields.next().and_then(|s| s.parse().ok()),
                    fields.next().and_then(|s| s.parse::<u64>().ok()),
                ) else {
                    return false;
                };
                stat(Path::new("/proc"), pid).is_some_and(|p| p.start == start)
            })
            .count()
            .min(super::MAX_AGENTS as usize) as u32,
    )
}

pub(super) fn clear(win: u64) {
    if let Some(path) = path(win) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;

    #[test]
    fn an_exited_background_session_disappears_without_another_hook() {
        let _s = Sandbox::new("agent-shell-exit");
        let thread = format!("g13map-test-{}", std::process::id());
        let mut child = std::process::Command::new("sleep")
            .arg("0.25")
            .env("CODEX_THREAD_ID", &thread)
            .env("CODEX_TOOL_CALL_ID", "background-probe")
            .spawn()
            .unwrap();
        refresh(424242, &thread);
        assert_eq!(count(424242), Some(1));
        child.wait().unwrap();
        // No refresh: the stored PID is checked at display-read time.
        assert_eq!(count(424242), Some(0));
        clear(424242);
        assert_eq!(count(424242), None);
    }

    #[test]
    fn wrappers_and_children_count_once_and_other_threads_do_not_count() {
        let _s = Sandbox::new("agent-shell-roots");
        let root = crate::config_dir().join("proc");
        let process = |pid, parent, start, thread, call| {
            let dir = root.join(format!("{pid}"));
            fs::create_dir_all(&dir).unwrap();
            let mut fields = vec!["0".to_string(); 20];
            fields[0] = "S".into();
            fields[1] = format!("{parent}");
            fields[19] = format!("{start}");
            fs::write(
                dir.join("stat"),
                format!("{pid} (shell with spaces) {}", fields.join(" ")),
            )
            .unwrap();
            fs::write(
                dir.join("environ"),
                format!("CODEX_THREAD_ID={thread}\0CODEX_TOOL_CALL_ID={call}\0"),
            )
            .unwrap();
        };
        process(101, 1, 77, "root", "a");
        process(102, 101, 78, "root", "a");
        process(201, 1, 88, "root", "b");
        process(301, 1, 99, "other", "c");
        process(401, 1, 100, "root", "observer");
        assert_eq!(
            roots(&root, "root", 401),
            vec![
                Process {
                    pid: 101,
                    parent: 1,
                    start: 77
                },
                Process {
                    pid: 201,
                    parent: 1,
                    start: 88
                }
            ]
        );
        // A PID reused by the OS must not keep the old session alive.
        process(101, 1, 120, "root", "new");
        assert_ne!(stat(&root, 101).unwrap().start, 77);
    }
}
