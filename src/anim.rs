// SPDX-License-Identifier: GPL-3.0-or-later
//! g13map-anim: the built-in pixel-art animations (`art`) as kept LCD pictures.
use g13map::{
    art::{Scene, SCENES},
    lcd,
};
use std::{env, fs, path::Path};

const HELP: &str = "g13map-anim — built-in LCD animations, drawn in code

  g13map-anim                    list the scenes
  g13map-anim keep [NAME...]     keep scenes as LCD pictures (all of them without names);
                                 the editor and `g13map profile lcd` can then use them
  g13map-anim pbm NAME DIR       write every frame of NAME to DIR as PBM files, lit white
  g13map-anim --version

A scene never overwrites a kept picture converted from a file, or kept text, of its name.";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        [] => Ok(list()),
        ["--version"] => Ok(format!(
            "g13pad {} (g13map-anim)",
            env!("CARGO_PKG_VERSION")
        )),
        ["--help" | "-h"] => Ok(HELP.to_string()),
        ["keep", names @ ..] => keep(names),
        ["pbm", name, dir] => pbm(name, Path::new(dir)),
        _ => Err("unknown arguments; see --help".to_string()),
    };
    match result {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("g13map-anim: {e}");
            std::process::exit(1);
        }
    }
}

fn find(name: &str) -> Result<&'static Scene, String> {
    Scene::find(name).ok_or_else(|| format!("no scene '{name}'; the list is `g13map-anim`"))
}

fn list() -> String {
    SCENES
        .iter()
        .map(|s| {
            let n = s.animation().frames.len();
            format!(
                "{:<14}{:>4} frames {:>4} ms {:>5.1} s  {}",
                s.name,
                n,
                s.delay_ms,
                n as f32 * s.delay_ms as f32 / 1000.0,
                s.about
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn keep(names: &[&str]) -> Result<String, String> {
    let scenes: Vec<&Scene> = if names.is_empty() {
        SCENES.iter().collect()
    } else {
        names.iter().map(|n| find(n)).collect::<Result<_, _>>()?
    };
    // Every name is checked before anything is written: a refusal leaves nothing half done.
    for s in &scenes {
        if let Some(what) = lcd::origin(s.name) {
            return Err(format!("'{}' is {what}; nothing overwritten", s.name));
        }
    }
    let mut out = Vec::new();
    for s in scenes {
        lcd::keep_animation(s.name, &s.animation())?;
        out.push(format!("kept {} ({})", s.name, lcd::path(s.name).display()));
    }
    Ok(out.join("\n"))
}

/// Frames as binary PBM (P4): a set PBM bit is black, so lit panel pixels come out white.
fn pbm(name: &str, dir: &Path) -> Result<String, String> {
    let scene = find(name)?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let frames = scene.animation().frames;
    for (i, (_, bm)) in frames.iter().enumerate() {
        let mut bytes = format!("P4\n{} {}\n", lcd::W, lcd::H).into_bytes();
        for y in 0..lcd::H {
            for xb in 0..lcd::W.div_ceil(8) {
                let mut b = 0u8;
                for bit in 0..8 {
                    let x = xb * 8 + bit;
                    if x < lcd::W && !bm.get(x, y) {
                        b |= 0x80 >> bit;
                    }
                }
                bytes.push(b);
            }
        }
        let p = dir.join(format!("{name}-{i:03}.pbm"));
        fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(format!(
        "{} frames of {name} in {}",
        frames.len(),
        dir.display()
    ))
}
