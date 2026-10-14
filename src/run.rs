// SPDX-License-Identifier: GPL-3.0-or-later
//! A program on a key (0.2.98). `bind KEY >RUN:KEY;` has the driver write the key's name
//! to its output pipe on press; `g13map watch` looks the key up in the active profile's
//! `# run KEY PROGRAM [ARGS...]` line and starts that program. The pipe token carries the
//! key, never the command: the pipe is group-writable and a profile is the user's own file,
//! so a stray writer can at most press a key the user already set up. Press only: the
//! driver's pipe-out action has no release half (docs/notes/run-bindings.md).
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The bind-file action that runs `key`'s program.
pub fn binding(key: &str) -> String {
    format!(">RUN:{key};")
}

/// The key whose program a saved action runs, when it is a run binding.
pub fn key_of_binding(action: &str) -> Option<&str> {
    action
        .strip_prefix(">RUN:")?
        .strip_suffix(';')
        .filter(|k| is_key(k))
}

/// The key named by a token from the driver's output pipe (`RUN:G5`; the `;` is already cut).
pub fn key_of_token(token: &str) -> Option<&str> {
    token.strip_prefix("RUN:").filter(|k| is_key(k))
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

/// Starts `key`'s program under profile `profile`. Through `systemd-run --user` when the
/// session has a manager: the program gets a transient unit of its own, so it outlives a
/// watcher restart and logs to the journal under its own name. Otherwise, or if that fails,
/// as a child of the watcher, reaped by a thread. Returns a line for the watcher's log.
pub fn start(key: &str, profile: &str, program: &Program) -> Result<String, String> {
    let path = program.resolve()?;
    let args = &program.words[1..];
    let env = [
        ("G13MAP_KEY", key.to_string()),
        ("G13MAP_PROFILE", profile.to_string()),
    ];
    let home = home();
    if unit_management() {
        let mut c = Command::new("systemd-run");
        c.args(["--user", "--quiet", "--collect"]).arg(format!(
            "--description=g13map run {key}: {}",
            program.line()
        ));
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
            Ok(st) if st.success() => {
                return Ok(format!(
                    "run {key}: {} (a unit of the session)",
                    program.line()
                ))
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
    let mut child = c.spawn().map_err(|e| format!("{}: {e}", path.display()))?;
    let (who, name) = (key.to_string(), program.name().to_string());
    std::thread::Builder::new()
        .name(format!("run {key}"))
        .spawn(move || match child.wait() {
            Ok(st) if !st.success() => eprintln!("g13map watch: run {who} ({name}): {st}"),
            Err(e) => eprintln!("g13map watch: run {who} ({name}): {e}"),
            Ok(_) => {}
        })
        .map_err(|e| format!("reaper thread: {e}"))?;
    Ok(format!(
        "run {key}: {} (a child of the watcher)",
        program.line()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn bindings_and_tokens_name_a_key_and_nothing_else() {
        assert_eq!(binding("G5"), ">RUN:G5;");
        assert_eq!(key_of_binding(">RUN:G5;"), Some("G5"));
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
        assert_eq!(key_of_token("RUN:G5"), Some("G5"));
        assert_eq!(key_of_token("RUN:"), None);
        assert_eq!(
            key_of_token("RUN:G5;"),
            None,
            "a token is read with its `;` cut"
        );
        assert_eq!(key_of_token("M1"), None);
        assert_eq!(key_of_token("AUDIO:mic-toggle"), None);
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

    /// The program sees the key and the profile in its environment and its own arguments.
    /// Hand-derived expectation: the script prints `$G13MAP_KEY $G13MAP_PROFILE $1` to `$2`.
    #[test]
    fn a_started_program_gets_the_key_the_profile_and_its_arguments() {
        let sb = Sandbox::new("run-start"); // G13MAP_UNIT=0: the child path, no systemd
        let script = sb.dir.join("mark.sh");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s %s %s' \"$G13MAP_KEY\" \"$G13MAP_PROFILE\" \"$1\" > \"$2\"\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let out = sb.dir.join("out.txt");
        let line = format!("{} hello {}", script.display(), out.display());
        let program = Program::parse(&line).unwrap();
        let said = start("G5", "gaming", &program).unwrap();
        assert!(said.contains("a child of the watcher"), "{said}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let got = loop {
            if let Ok(s) = fs::read_to_string(&out) {
                break s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the program never wrote"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(got, "G5 gaming hello");
        let gone = Program::parse(sb.dir.join("gone").to_str().unwrap()).unwrap();
        assert!(start("G5", "gaming", &gone).is_err());
    }
}
