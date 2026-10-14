// SPDX-License-Identifier: GPL-3.0-or-later
//! A program on a key (0.2.98; release actions and hold since 0.3.0). `bind KEY >RUN:KEY;`
//! has the driver write the key's name to its output pipe on press, and `bind KEY >RUN:KEY;
//! RELEASE:KEY;` on release as well; `g13map watch` looks the key up in the active
//! profile's `# run KEY [WHEN] PROGRAM [ARGS...]` line and starts (or, for `hold`, stops)
//! that program. The pipe token carries the key, never the command: the pipe is
//! group-writable and a profile is the user's own file, so a stray writer can at most
//! press a key the user already set up (docs/notes/run-bindings.md).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// When a key's program runs: the word in the `# run` line (never written for `Pressed`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum When {
    #[default]
    Pressed,
    Released,
    Both,
    /// Started on press; its unit (or child) is stopped on release.
    Hold,
}

impl When {
    pub const ALL: [When; 4] = [When::Pressed, When::Released, When::Both, When::Hold];
    /// The word in the profile and on the command line.
    pub fn word(self) -> &'static str {
        match self {
            When::Pressed => "pressed",
            When::Released => "released",
            When::Both => "both",
            When::Hold => "hold",
        }
    }
    pub fn parse(word: &str) -> Option<When> {
        When::ALL.into_iter().find(|w| w.word() == word)
    }
    /// The editor's radio label.
    pub fn label(self) -> &'static str {
        match self {
            When::Pressed => "Pressed",
            When::Released => "Released",
            When::Both => "Both",
            When::Hold => "Hold",
        }
    }
    /// The editor's hover text: what the program gets and when.
    pub fn hover(self) -> &'static str {
        match self {
            When::Pressed => "Starts the program when the key goes down (G13MAP_EDGE=press).",
            When::Released => "Starts the program when the key comes up (G13MAP_EDGE=release).",
            When::Both => {
                "Starts the program on both ends of the key; it reads G13MAP_EDGE (press or \
                 release) to tell which."
            }
            When::Hold => {
                "Starts the program when the key goes down and stops it (SIGTERM to its unit) \
                 when the key comes up: a recording, a microphone, a light, while held."
            }
        }
    }
    /// Whether the driver has to report the release as well as the press.
    pub fn needs_release(self) -> bool {
        self != When::Pressed
    }
}

/// Which end of the key a pipe token reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Press,
    Release,
}

impl Edge {
    /// The value of `G13MAP_EDGE` in the program's environment.
    pub fn word(self) -> &'static str {
        match self {
            Edge::Press => "press",
            Edge::Release => "release",
        }
    }
}

/// A `# run` line after the key: when, and the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub when: When,
    pub program: Program,
}

impl Run {
    /// `[pressed|released|both|hold] PROGRAM [ARGS...]`. The first word is a mode word
    /// only when something follows it; a program that is itself named like one of the
    /// words and takes arguments is given as a path (`./both`).
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (when, rest) = match text.split_once(char::is_whitespace) {
            Some((w, rest)) if When::parse(w).is_some() => (When::parse(w).unwrap(), rest),
            _ => (When::Pressed, text),
        };
        Ok(Self {
            when,
            program: Program::parse(rest)?,
        })
    }
    /// The line as saved after the key: the mode word unless it is `pressed`.
    pub fn line(&self) -> String {
        match self.when {
            When::Pressed => self.program.line(),
            w => format!("{} {}", w.word(), self.program.line()),
        }
    }
}

/// The bind-file action that runs `key`'s program: on press, or press and release.
pub fn binding(key: &str, when: When) -> String {
    if when.needs_release() {
        format!(">RUN:{key}; RELEASE:{key};")
    } else {
        format!(">RUN:{key};")
    }
}

/// The key whose program a saved action runs, when it is a run binding (either shape).
pub fn key_of_binding(action: &str) -> Option<&str> {
    let (press, release) = match action.split_once(' ') {
        Some((p, r)) => (p, Some(r)),
        None => (action, None),
    };
    let key = press
        .strip_prefix(">RUN:")?
        .strip_suffix(';')
        .filter(|k| is_key(k))?;
    match release {
        None => Some(key),
        Some(r) if r == format!("RELEASE:{key};") => Some(key),
        Some(_) => None,
    }
}

/// Whether a saved run action reports the release too.
pub fn binding_reports_release(action: &str) -> bool {
    key_of_binding(action).is_some() && action.contains(" RELEASE:")
}

/// The key and edge named by a token from the driver's output pipe (`RUN:G5` or
/// `RELEASE:G5`; the `;` is already cut).
pub fn token(token: &str) -> Option<(&str, Edge)> {
    if let Some(k) = token.strip_prefix("RUN:") {
        return is_key(k).then_some((k, Edge::Press));
    }
    token
        .strip_prefix("RELEASE:")
        .filter(|k| is_key(k))
        .map(|k| (k, Edge::Release))
}

fn is_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 16 && k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Where bare program names are looked for before `$PATH`: the user's scripts, then the
/// ones shipped with the package.
pub fn dirs() -> [PathBuf; 2] {
    [
        crate::config_dir().join("scripts"),
        PathBuf::from("/usr/share/g13pad/scripts"),
    ]
}

/// The executables in `dirs()`, by name, for the editor to offer.
pub fn listed() -> Vec<String> {
    let mut names: Vec<String> = dirs()
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| executable(&e.path()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A path for a `# run` line: under the home directory it starts with `~`.
pub fn tilde(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) if !home().as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// What a `# run` line names: the program and its arguments, split on whitespace (a path
/// with spaces goes behind a wrapper script or a symlink).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub words: Vec<String>,
}

impl Program {
    /// One line, at least one word, no control characters (a tab is a separator).
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.chars().any(|c| c.is_control() && c != '\t') {
            return Err("a program line is one line without control characters".into());
        }
        let words: Vec<String> = text.split_whitespace().map(String::from).collect();
        if words.is_empty() {
            return Err("no program named".into());
        }
        Ok(Self { words })
    }
    /// The line as saved: single spaces.
    pub fn line(&self) -> String {
        self.words.join(" ")
    }
    /// The program's own name, for a key label.
    pub fn name(&self) -> &str {
        Path::new(&self.words[0])
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.words[0])
    }
    /// Where the program is. A word with a `/` is a path (`~` is home); a bare name is
    /// looked up in `dirs()`, then on `$PATH`. Either way it must be an executable file.
    pub fn resolve(&self) -> Result<PathBuf, String> {
        let word = &self.words[0];
        if word.contains('/') {
            let p = expand_home(word, &home());
            return if executable(&p) {
                Ok(p)
            } else if p.exists() {
                Err(format!("{} is not an executable file", p.display()))
            } else {
                Err(format!("{} does not exist", p.display()))
            };
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        for d in dirs().into_iter().chain(std::env::split_paths(&path)) {
            let p = d.join(word);
            if executable(&p) {
                return Ok(p);
            }
        }
        let [user, shipped] = dirs();
        Err(format!(
            "no executable {word} in {}, {} or on PATH",
            user.display(),
            shipped.display()
        ))
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// `~` and `~/x` are under the home directory; anything else is itself.
fn expand_home(word: &str, home: &Path) -> PathBuf {
    match word.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(word),
    }
}

fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Whether to go through the user's systemd manager (off in tests and under `G13MAP_UNIT=0`).
fn unit_management() -> bool {
    !std::env::var("G13MAP_UNIT").is_ok_and(|v| v == "0")
}

/// The transient unit a held program runs as, so the release can stop it by name.
fn hold_unit(key: &str) -> String {
    format!("g13map-hold-{key}")
}

/// How a program was started: as a unit of the session, or as a child of the watcher.
enum Started {
    Unit,
    Child(Child),
}

/// Starts `key`'s program under profile `profile`, with `G13MAP_EDGE` set to `edge`. Through
/// `systemd-run --user` when the session has a manager (named `unit` when given, else
/// anonymous): the program gets a transient unit of its own, so it outlives a watcher
/// restart and logs to the journal under its own name. Otherwise, or if that fails, as a
/// child of the watcher. Returns how it was started, or why not.
fn launch(
    key: &str,
    profile: &str,
    program: &Program,
    edge: Edge,
    unit: Option<&str>,
) -> Result<Started, String> {
    let path = program.resolve()?;
    let args = &program.words[1..];
    let env = [
        ("G13MAP_KEY", key.to_string()),
        ("G13MAP_PROFILE", profile.to_string()),
        ("G13MAP_EDGE", edge.word().to_string()),
    ];
    let home = home();
    if unit_management() {
        let mut c = Command::new("systemd-run");
        c.args(["--user", "--quiet", "--collect"]).arg(format!(
            "--description=g13map run {key}: {}",
            program.line()
        ));
        if let Some(u) = unit {
            c.arg(format!("--unit={u}"));
        }
        if !home.as_os_str().is_empty() {
            c.arg(format!("--working-directory={}", home.display()));
        }
        for (k, v) in &env {
            c.arg(format!("--setenv={k}={v}"));
        }
        // What the watcher has of the session goes along, in case the manager lacks it.
        for k in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if std::env::var_os(k).is_some() {
                c.arg(format!("--setenv={k}"));
            }
        }
        c.arg("--")
            .arg(&path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        match c.status() {
            Ok(st) if st.success() => return Ok(Started::Unit),
            // A named unit still running means the key is still held: not a second start.
            Ok(_) if unit.is_some() => {
                return Err(format!("{} is still running", program.name()));
            }
            Ok(st) => eprintln!(
                "g13map watch: systemd-run {st}; running {} as a child instead",
                program.name()
            ),
            Err(e) => eprintln!(
                "g13map watch: systemd-run: {e}; running {} as a child instead",
                program.name()
            ),
        }
    }
    let mut c = Command::new(&path);
    c.args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null());
    if !home.as_os_str().is_empty() {
        c.current_dir(&home);
    }
    c.spawn()
        .map(Started::Child)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Waits for a child on a thread of its own, reporting a failure to the log.
fn reap(key: &str, program: &Program, mut child: Child) -> Result<(), String> {
    let (who, name) = (key.to_string(), program.name().to_string());
    std::thread::Builder::new()
        .name(format!("run {key}"))
        .spawn(move || match child.wait() {
            Ok(st) if !st.success() => eprintln!("g13map watch: run {who} ({name}): {st}"),
            Err(e) => eprintln!("g13map watch: run {who} ({name}): {e}"),
            Ok(_) => {}
        })
        .map(|_| ())
        .map_err(|e| format!("reaper thread: {e}"))
}

/// Starts `key`'s program for one edge of the key. Returns a line for the watcher's log.
pub fn start(key: &str, profile: &str, program: &Program, edge: Edge) -> Result<String, String> {
    let how = match launch(key, profile, program, edge, None)? {
        Started::Unit => "a unit of the session",
        Started::Child(child) => {
            reap(key, program, child)?;
            "a child of the watcher"
        }
    };
    Ok(format!(
        "run {key} ({}): {} ({how})",
        edge.word(),
        program.line()
    ))
}

/// The programs held down: started on press, stopped on release. One per key.
#[derive(Default)]
pub struct Holds {
    children: HashMap<String, Child>,
}

impl Holds {
    /// The press: starts the program unless the key's last one is still running.
    pub fn press(&mut self, key: &str, profile: &str, program: &Program) -> Result<String, String> {
        if let Some(c) = self.children.get_mut(key) {
            match c.try_wait() {
                Ok(None) => return Err(format!("{} is still running", program.name())),
                _ => {
                    self.children.remove(key);
                }
            }
        }
        let unit = hold_unit(key);
        let how = match launch(key, profile, program, Edge::Press, Some(&unit))? {
            Started::Unit => "a unit of the session",
            Started::Child(child) => {
                self.children.insert(key.to_string(), child);
                "a child of the watcher"
            }
        };
        Ok(format!("hold {key}: {} ({how})", program.line()))
    }
    /// The release: stops the key's program, by unit name or by SIGTERM to the child.
    pub fn release(&mut self, key: &str) -> Result<String, String> {
        if let Some(child) = self.children.remove(key) {
            let pid = child.id();
            // SIGTERM, not `Child::kill`'s SIGKILL: the program gets to finish its file.
            let r = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            if r != 0 {
                return Err(format!(
                    "stopping pid {pid}: {}",
                    std::io::Error::last_os_error()
                ));
            }
            std::thread::Builder::new()
                .name(format!("hold {key}"))
                .spawn(move || {
                    let mut child = child;
                    let _ = child.wait();
                })
                .map_err(|e| format!("reaper thread: {e}"))?;
            return Ok(format!("hold {key}: stopped pid {pid}"));
        }
        if !unit_management() {
            return Err("nothing held".into());
        }
        let unit = hold_unit(key);
        let st = Command::new("systemctl")
            .args(["--user", "stop", &unit])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("systemctl: {e}"))?;
        if st.success() {
            Ok(format!("hold {key}: stopped {unit}"))
        } else {
            Err(format!("systemctl --user stop {unit}: {st}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn bindings_and_tokens_name_a_key_and_nothing_else() {
        assert_eq!(binding("G5", When::Pressed), ">RUN:G5;");
        assert_eq!(binding("G5", When::Released), ">RUN:G5; RELEASE:G5;");
        assert_eq!(binding("G5", When::Both), ">RUN:G5; RELEASE:G5;");
        assert_eq!(binding("G5", When::Hold), ">RUN:G5; RELEASE:G5;");
        assert_eq!(key_of_binding(">RUN:G5;"), Some("G5"));
        assert_eq!(key_of_binding(">RUN:G5; RELEASE:G5;"), Some("G5"));
        assert_eq!(
            key_of_binding(">RUN:STICK_PAGEDOWN;"),
            Some("STICK_PAGEDOWN")
        );
        assert_eq!(
            key_of_binding(">RUN:G5"),
            None,
            "the driver's `;` terminator"
        );
        assert_eq!(key_of_binding(">RUN:;"), None);
        assert_eq!(key_of_binding(">RUN:../x;"), None);
        assert_eq!(key_of_binding(">AUDIO:mic-toggle;"), None);
        assert_eq!(key_of_binding("KEY_A"), None);
        assert_eq!(
            key_of_binding(">RUN:G5; RELEASE:G6;"),
            None,
            "the two halves must name the same key"
        );
        assert_eq!(key_of_binding(">RUN:G5; RELEASE:G5"), None);
        assert_eq!(key_of_binding(">RUN:G5; KEY_A"), None);
        assert!(binding_reports_release(">RUN:G5; RELEASE:G5;"));
        assert!(!binding_reports_release(">RUN:G5;"));
        assert!(!binding_reports_release("KEY_A RELEASE:G5;"));
        assert_eq!(token("RUN:G5"), Some(("G5", Edge::Press)));
        assert_eq!(token("RELEASE:G5"), Some(("G5", Edge::Release)));
        assert_eq!(token("RUN:"), None);
        assert_eq!(token("RELEASE:"), None);
        assert_eq!(token("RUN:G5;"), None, "a token is read with its `;` cut");
        assert_eq!(token("M1"), None);
        assert_eq!(token("AUDIO:mic-toggle"), None);
    }

    /// The mode word is optional and never written for `pressed`; a program that is itself
    /// named like a mode word is given as a path.
    #[test]
    fn run_lines_carry_an_optional_mode_word() {
        let r = Run::parse("alacritty -e htop").unwrap();
        assert_eq!(
            (r.when, r.line()),
            (When::Pressed, "alacritty -e htop".into())
        );
        let r = Run::parse(" pressed  alacritty ").unwrap();
        assert_eq!((r.when, r.line()), (When::Pressed, "alacritty".into()));
        let r = Run::parse("hold arecord -f cd ~/clip.wav").unwrap();
        assert_eq!(r.when, When::Hold);
        assert_eq!(r.program.words, ["arecord", "-f", "cd", "~/clip.wav"]);
        assert_eq!(r.line(), "hold arecord -f cd ~/clip.wav");
        for (text, when) in [("released x", When::Released), ("both x", When::Both)] {
            assert_eq!(Run::parse(text).unwrap().when, when, "{text}");
        }
        assert_eq!(Run::parse("./both").unwrap().program.words, ["./both"]);
        assert_eq!(
            Run::parse("hold both").unwrap().program.words,
            ["both"],
            "the second word is the program, whatever it is called"
        );
        assert_eq!(
            Run::parse("released hold").unwrap(),
            Run {
                when: When::Released,
                program: Program::parse("hold").unwrap()
            },
            "one mode word, then the program"
        );
        assert_eq!(
            Run::parse("hold ").unwrap(),
            Run {
                when: When::Pressed,
                program: Program::parse("hold").unwrap()
            },
            "a lone word is the program, whatever it is called"
        );
        for bad in ["", "   ", "hold\u{7}x"] {
            assert!(Run::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(When::parse("hold"), Some(When::Hold));
        assert_eq!(When::parse("Hold"), None);
        for w in When::ALL {
            assert_eq!(When::parse(w.word()), Some(w));
        }
        assert_eq!(Edge::Press.word(), "press");
        assert_eq!(Edge::Release.word(), "release");
    }

    /// A held program is started on press, refused a second start while it runs, and sent
    /// SIGTERM on release so it can finish: hand-derived file content `started`, `stopped`.
    #[test]
    fn a_held_program_is_stopped_on_release_with_sigterm() {
        let sb = Sandbox::new("run-hold"); // G13MAP_UNIT=0: the child path, no systemd
        let script = sb.dir.join("held.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho started >> \"$1\"\ntrap 'echo stopped >> \"$1\"; exit 0' TERM\n\
             while :; do sleep 0.05; done\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let out = sb.dir.join("held.txt");
        let program = Program::parse(&format!("{} {}", script.display(), out.display())).unwrap();
        let wait_for = |want: &str| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if fs::read_to_string(&out).is_ok_and(|s| s == want) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "waiting for {want:?}");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        let mut holds = Holds::default();
        assert!(holds.release("G5").is_err(), "nothing held yet");
        let said = holds.press("G5", "gaming", &program).unwrap();
        assert!(said.contains("a child of the watcher"), "{said}");
        wait_for("started\n");
        let e = holds.press("G5", "gaming", &program).unwrap_err();
        assert!(e.contains("still running"), "{e}");
        let said = holds.release("G5").unwrap();
        assert!(said.starts_with("hold G5: stopped pid "), "{said}");
        wait_for("started\nstopped\n");
        assert!(holds.release("G5").is_err(), "released twice");
        // After it ended on its own, a press starts it again.
        holds.press("G5", "gaming", &program).unwrap();
        wait_for("started\nstopped\nstarted\n");
        holds.release("G5").unwrap();
        wait_for("started\nstopped\nstarted\nstopped\n");
    }

    #[test]
    fn program_lines_are_words() {
        let p = Program::parse("  alacritty \t -e  htop ").unwrap();
        assert_eq!(p.words, ["alacritty", "-e", "htop"]);
        assert_eq!(p.line(), "alacritty -e htop");
        assert_eq!(p.name(), "alacritty");
        assert_eq!(
            Program::parse("~/bin/lights.sh on").unwrap().name(),
            "lights.sh"
        );
        assert!(Program::parse("").is_err());
        assert!(Program::parse("   ").is_err());
        assert!(Program::parse("a\u{7}b").is_err());
        assert!(Program::parse("a\nb").is_err());
        let elsewhere = Path::new("/home/someone");
        assert_eq!(expand_home("~", elsewhere), PathBuf::from("/home/someone"));
        assert_eq!(
            expand_home("~/bin/x", elsewhere),
            PathBuf::from("/home/someone/bin/x")
        );
        assert_eq!(
            expand_home("~other/x", elsewhere),
            PathBuf::from("~other/x")
        );
        assert_eq!(
            expand_home("/usr/bin/x", elsewhere),
            PathBuf::from("/usr/bin/x")
        );
        if !home().as_os_str().is_empty() {
            assert_eq!(tilde(&home().join("bin/x")), "~/bin/x");
        }
        assert_eq!(tilde(Path::new("/usr/bin/x")), "/usr/bin/x");
    }

    /// A bare name resolves from the user's scripts folder before PATH; a file without the
    /// executable bit and a missing path are refused with the reason.
    #[test]
    fn programs_resolve_from_the_scripts_folder_then_path_and_must_be_executable() {
        let sb = Sandbox::new("run-resolve");
        let scripts = crate::config_dir().join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        let mine = scripts.join("sh");
        fs::write(&mine, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&mine, fs::Permissions::from_mode(0o755)).unwrap();
        // `sh` exists on PATH too; the user's own copy wins.
        assert_eq!(Program::parse("sh").unwrap().resolve().unwrap(), mine);
        assert!(Program::parse("true")
            .unwrap()
            .resolve()
            .unwrap()
            .ends_with("true"));
        let plain = scripts.join("plain");
        fs::write(&plain, "not a program").unwrap();
        fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
        let e = Program::parse("plain").unwrap().resolve().unwrap_err();
        assert!(e.contains("no executable plain"), "{e}");
        let e = Program::parse(plain.to_str().unwrap())
            .unwrap()
            .resolve()
            .unwrap_err();
        assert!(e.contains("not an executable file"), "{e}");
        let missing = sb.dir.join("nothing-here");
        let e = Program::parse(missing.to_str().unwrap())
            .unwrap()
            .resolve()
            .unwrap_err();
        assert!(e.contains("does not exist"), "{e}");
        let e = Program::parse("g13map-no-such-program-xyz")
            .unwrap()
            .resolve()
            .unwrap_err();
        assert!(e.contains("or on PATH"), "{e}");
        assert_eq!(listed(), vec!["sh".to_string()]);
    }

    /// The program sees the key, the profile and the edge in its environment and its own
    /// arguments. Hand-derived expectation: the script prints
    /// `$G13MAP_KEY $G13MAP_PROFILE $G13MAP_EDGE $1` to `$2`.
    #[test]
    fn a_started_program_gets_the_key_the_profile_the_edge_and_its_arguments() {
        let sb = Sandbox::new("run-start"); // G13MAP_UNIT=0: the child path, no systemd
        let script = sb.dir.join("mark.sh");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s %s %s %s' \"$G13MAP_KEY\" \"$G13MAP_PROFILE\" \
             \"$G13MAP_EDGE\" \"$1\" > \"$2\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let read_when_written = |out: &Path| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Ok(s) = fs::read_to_string(out) {
                    break s;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the program never wrote"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        for (edge, want) in [
            (Edge::Press, "G5 gaming press hello"),
            (Edge::Release, "G5 gaming release hello"),
        ] {
            let out = sb.dir.join(format!("{}.txt", edge.word()));
            let line = format!("{} hello {}", script.display(), out.display());
            let program = Program::parse(&line).unwrap();
            let said = start("G5", "gaming", &program, edge).unwrap();
            assert!(said.contains("a child of the watcher"), "{said}");
            assert!(said.contains(&format!("({})", edge.word())), "{said}");
            assert_eq!(read_when_written(&out), want);
        }
        let gone = Program::parse(sb.dir.join("gone").to_str().unwrap()).unwrap();
        assert!(start("G5", "gaming", &gone, Edge::Press).is_err());
    }
}
