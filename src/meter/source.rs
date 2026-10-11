// SPDX-License-Identifier: GPL-3.0-or-later
//! Source responsibilities for Health Mode.
use super::*;

// ---- Source engine games ----

/// The LCD page a Source 2013 client renders when started with `-g15`: the title page
/// says `G13 wait`, the player page spells the health line that `g15.so`
/// (`contrib/source-health`) turns into the feed.
pub const SOURCE_RES: &str = include_str!("../../contrib/source-health/g15.res");

/// A Source engine game folder as Steam lays it out: `bin/` with the engine (under
/// `linux64/` for a 64-bit client), and one or more mod folders with a `gameinfo.txt`
/// (`cstrike`, `hl2`, `episodic`, ...), each of which mounts `custom/*`.
#[derive(Debug)]
pub struct SourceGame {
    pub bits: u8,
    /// Where the engine looks for the module: `bin/g15.so` (32-bit) or
    /// `bin/linux64/bin/g15.so` (64-bit, its platform folder plus the name it asks for).
    pub module: PathBuf,
    pub mods: Vec<PathBuf>,
    /// From the mods' `gameinfo.txt` `type` keys: all `singleplayer_only` makes the game
    /// singleplayer; anything else, or nothing said, multiplayer, the safe side (the sp
    /// module patches the client's vtable for armour, which no anti-cheat should see).
    pub kind: SourceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Single,
    Multi,
}

impl SourceKind {
    pub fn word(self) -> &'static str {
        match self {
            SourceKind::Single => "sp",
            SourceKind::Multi => "mp",
        }
    }
    pub fn parse(word: &str) -> Option<SourceKind> {
        match word {
            "sp" | "single" | "singleplayer" => Some(SourceKind::Single),
            "mp" | "multi" | "multiplayer" => Some(SourceKind::Multi),
            _ => None,
        }
    }
}

/// The `type` key of a gameinfo.txt (`singleplayer_only`, `multiplayer_only`), if any.
pub(super) fn gameinfo_type(mod_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(mod_dir.join("gameinfo.txt")).ok()?;
    text.lines().find_map(|l| {
        let mut w = l.split_whitespace();
        (w.next()? == "type").then(|| w.next().unwrap_or("").trim_matches('"').to_string())
    })
}

pub fn source_game(root: &std::path::Path) -> Result<SourceGame, String> {
    use std::io::Read;
    let (engine, module) = if root.join("bin/linux64/engine.so").is_file() {
        (
            root.join("bin/linux64/engine.so"),
            root.join("bin/linux64/bin/g15.so"),
        )
    } else if root.join("bin/engine.so").is_file() {
        (root.join("bin/engine.so"), root.join("bin/g15.so"))
    } else {
        return Err(format!(
            "{}: no bin/engine.so or bin/linux64/engine.so here; give the game's own folder, \
             the one holding bin/ and the mod folder (cstrike, hl2, ...)",
            root.display()
        ));
    };
    let mut head = [0u8; 5];
    fs::File::open(&engine)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| format!("{}: {e}", engine.display()))?;
    let bits = match head {
        [0x7f, b'E', b'L', b'F', 1] => 32,
        [0x7f, b'E', b'L', b'F', 2] => 64,
        _ => return Err(format!("{}: not an ELF library", engine.display())),
    };
    let mut mods: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("gameinfo.txt").is_file())
        .collect();
    mods.sort();
    if mods.is_empty() {
        return Err(format!(
            "{}: no mod folder with a gameinfo.txt",
            root.display()
        ));
    }
    let types: Vec<String> = mods.iter().filter_map(|m| gameinfo_type(m)).collect();
    let kind = if types.len() == mods.len() && types.iter().all(|t| t == "singleplayer_only") {
        SourceKind::Single
    } else {
        SourceKind::Multi
    };
    Ok(SourceGame {
        bits,
        module,
        mods,
        kind,
    })
}

/// The built module for a client of `bits`: `G13MAP_G15_DIR`, then
/// `lib/g13pad/source-health` under the executable's prefix, then the usual prefixes.
pub(super) fn g15_module(kind: SourceKind, bits: u8) -> Result<PathBuf, String> {
    let name = format!(
        "g15-{}-{}.so",
        kind.word(),
        if bits == 64 { "x86_64" } else { "i386" }
    );
    let mut dirs = Vec::new();
    if let Some(d) = env::var_os("G13MAP_G15_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(prefix) = env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .and_then(|bin| bin.parent().map(Path::to_path_buf))
    }) {
        dirs.push(prefix.join("lib/g13pad/source-health"));
    }
    dirs.push(PathBuf::from("/usr/lib/g13pad/source-health"));
    dirs.push(PathBuf::from("/usr/local/lib/g13pad/source-health"));
    dirs.iter()
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "{name} not found in {}; build the source-health component or set G13MAP_G15_DIR",
                dirs.iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

pub(super) fn res_path(mod_dir: &Path) -> PathBuf {
    mod_dir.join("custom/g13pad/resource/g15.res")
}

/// `g13map health source DIR [sp|mp]`: the module (of the game's kind unless told
/// otherwise) into the game's bin folder, the page into every mod folder's `custom/`,
/// and what is left to do by hand.
pub fn source_install(root: &Path, kind: Option<SourceKind>) -> Result<String, String> {
    let game = source_game(root)?;
    let kind = kind.unwrap_or(game.kind);
    let module = g15_module(kind, game.bits)?;
    if let Some(d) = game.module.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // Beside, then renamed over: a running game keeps the inode it mapped.
    let staged = game.module.with_extension("so.new");
    fs::copy(&module, &staged)
        .and_then(|_| fs::rename(&staged, &game.module))
        .map_err(|e| format!("{} -> {}: {e}", module.display(), game.module.display()))?;
    let mut out = format!(
        "{}-bit {} client: {} ({})\n",
        game.bits,
        match kind {
            SourceKind::Single => "singleplayer",
            SourceKind::Multi => "multiplayer",
        },
        game.module.display(),
        match kind {
            SourceKind::Single => "health and armour; hooks the client's Battery message",
            SourceKind::Multi => "health only; touches nothing of the game's",
        }
    );
    for m in &game.mods {
        let res = res_path(m);
        let dir = res.parent().unwrap();
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let staged = res.with_extension("res.new");
        fs::write(&staged, SOURCE_RES)
            .and_then(|_| fs::rename(&staged, &res))
            .map_err(|e| format!("{}: {e}", res.display()))?;
        out.push_str(&format!("page: {}\n", res.display()));
    }
    out.push_str(
        "Now add -g15 to the game's launch options in Steam, and put its profile in health \
         mode feed (g13map profile health NAME feed). Remove with: g13map health source DIR remove",
    );
    Ok(out)
}

/// `g13map health source DIR remove`: only what `source_install` put there.
pub fn source_remove(root: &Path) -> Result<String, String> {
    let game = source_game(root)?;
    let mut removed = Vec::new();
    let mut gone = |p: &Path| -> Result<(), String> {
        match fs::remove_file(p) {
            Ok(()) => {
                removed.push(p.display().to_string());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    };
    gone(&game.module)?;
    for m in &game.mods {
        let res = res_path(m);
        gone(&res)?;
        // Our folders only, and only while empty.
        let _ = fs::remove_dir(res.parent().unwrap());
        let _ = fs::remove_dir(m.join("custom/g13pad"));
    }
    if game.bits == 64 {
        let _ = fs::remove_dir(game.module.parent().unwrap());
    }
    Ok(if removed.is_empty() {
        "nothing of ours there".into()
    } else {
        format!("removed:\n{}", removed.join("\n"))
    })
}
