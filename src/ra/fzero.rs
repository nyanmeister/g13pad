// SPDX-License-Identifier: GPL-3.0-or-later
//! F-Zero GX USA telemetry from Dolphin's read-only shared emulated RAM.
//! Addresses: RA game 9699 public CodeNotes, pointer at 0x30c8 (2026-10-09).
//! RAM bank zero starts at backing-file offset zero (Dolphin Memmap.cpp).
use super::{process_start, progress_bar, text, Bitmap, Model, Session};
use crate::draw::Draw;
use std::{
    fs::{self, File},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const GAME: u32 = 9699;
const RAM_SIZE: u32 = 0x1800000;
const ROOT: u64 = 0x30c8;
const MODE: u32 = 0x802453e1;
const FLAGS: u32 = 0x801bdc71;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Ready,
    Racing,
    Finished,
    Retired,
}
#[derive(Debug)]
pub(super) struct Snapshot {
    mode: u8,
    difficulty: u8,
    pilot: u8,
    cup: u8,
    race: u8,
    chapter: u8,
    phase: Phase,
    speed: Option<f32>,
    energy: Option<f32>,
    boosting: bool,
    setup: Option<String>,
    stats: Option<[f32; 4]>,
    ceiling: Option<f32>,
}

/// Bounds apply to real GameCube RAM, not the larger allocation or other banks.
fn address(root: u32, offset: u32, width: usize) -> Option<u64> {
    let mut address = root.wrapping_add(offset);
    if (0x80000000..0x81800000).contains(&address) {
        address -= 0x80000000;
    }
    (u64::from(address) + width as u64 <= u64::from(RAM_SIZE)).then_some(u64::from(address))
}
fn read<const N: usize>(file: &File, offset: u64) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    file.read_exact_at(&mut bytes, offset).ok()?;
    Some(bytes)
}
fn decode(file: &File) -> Option<Snapshot> {
    // The disc id is part of the binding, not only a window title. The revision is not:
    // a newer revision is attempted under the same validation (asked 2026-10-13), and a
    // moved layout fails the pointer and bounds checks below rather than this line.
    if read::<6>(file, 0)? != *b"GFZE01" {
        return None;
    }
    let root = u32::from_be_bytes(read(file, ROOT)?);
    if !(0x80000000..0x81800000).contains(&root) || root & 3 != 0 {
        return None;
    }
    let byte = |offset| read::<1>(file, address(root, offset, 1)?).map(|b| b[0]);
    let mode = byte(MODE)?;
    if !(1..=4).contains(&mode) {
        return None; // Menus, replays and other screens use the RA overview.
    }
    let flags = read::<2>(file, address(root, FLAGS, 2)?)?;
    let phase = if flags[1] & 0x20 != 0 || flags[0] & 8 != 0 {
        Phase::Retired
    } else if flags[0] & 1 != 0 || flags[1] & 0x10 != 0 {
        Phase::Finished
    } else if flags[0] & 0x10 != 0 {
        Phase::Racing
    } else {
        Phase::Ready
    };
    let difficulty = byte(if mode == 4 { 0x801ba0dd } else { 0x802453e9 })?;
    let pilot = byte(0x801bddc4)?;
    let cup = byte(0x801bdd16)?;
    let race = byte(0x801bdd17)?;
    let chapter = byte(0x801ba0dc)?;
    if pilot > 40
        || (mode == 1 && (difficulty > 3 || !(1..=5).contains(&cup) || race > 5))
        || (mode == 4 && (difficulty > 2 || chapter > 8))
    {
        return None;
    }
    // The first entrant is the player. Read a bounded block in one operation;
    // pointer validation also prevents loading/menu data becoming a fake gauge.
    let pointer_at = address(root, 0x227878, 4)?;
    let pointer = u32::from_be_bytes(read(file, pointer_at)?);
    let telemetry = if phase == Phase::Racing {
        if !(0x80000000..0x81800000).contains(&pointer) || pointer & 3 != 0 {
            return None;
        }
        let block = read::<0x620>(file, address(pointer, 0, 0x620)?)?;
        let word = |at| u16::from_be_bytes([block[at], block[at + 1]]);
        let float = |at| f32::from_be_bytes(block[at..at + 4].try_into().unwrap());
        let machine = word(6);
        let speed = float(0x17c);
        let energy = float(0x184);
        let maximum = f32::from(word(0x5da));
        let stats = [float(8), float(0x220), float(0x22c), float(0x23c)];
        if word(4) != 0
            || machine > 255
            || (machine <= 40 && machine != u16::from(pilot))
            || !speed.is_finite()
            || !(0.0..=100000.0).contains(&speed)
            || !maximum.is_finite()
            || !(1.0..=10000.0).contains(&maximum)
            || !energy.is_finite()
            || !(0.0..=maximum).contains(&energy)
            || stats
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0 || *v > 100000.0)
        {
            return None;
        }
        let key = format!(
            "{machine}-{:08x}-{:08x}-{:08x}-{:08x}",
            stats[0].to_bits(),
            stats[1].to_bits(),
            stats[2].to_bits(),
            stats[3].to_bits()
        );
        Some((
            speed,
            energy * 100.0 / maximum,
            block[0x18a] > 0,
            key,
            stats,
        ))
    } else {
        None // Racing-only values retain stale data on results and after retirement.
    };
    // Reject a sample crossing a pointer, screen or race-state transition.
    if read::<4>(file, ROOT)? != root.to_be_bytes()
        || byte(MODE)? != mode
        || read::<2>(file, address(root, FLAGS, 2)?)? != flags
        || read::<6>(file, 0)? != *b"GFZE01"
        || read::<4>(file, pointer_at)? != pointer.to_be_bytes()
    {
        return None;
    }
    let (speed, energy, boosting, setup, stats) = telemetry.map_or(
        (None, None, false, None, None),
        |(speed, energy, boost, setup, stats)| {
            (Some(speed), Some(energy), boost, Some(setup), Some(stats))
        },
    );
    Some(Snapshot {
        mode,
        difficulty,
        pilot,
        cup,
        race,
        chapter,
        phase,
        speed,
        energy,
        boosting,
        setup,
        stats,
        ceiling: None,
    })
}

struct Ram {
    file: File,
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Ram {
    fn discover(proc_root: &Path, pid: u32) -> Option<Self> {
        let expected = format!("/dev/shm/dolphin-emu.{pid}");
        for entry in fs::read_dir(proc_root.join(pid.to_string()).join("fd"))
            .ok()?
            .flatten()
        {
            let path = entry.path();
            let Ok(target) = fs::read_link(&path) else {
                continue;
            };
            let Some(target) = target.to_str() else {
                continue;
            };
            if target != expected && target != format!("{expected} (deleted)") {
                continue;
            }
            let Ok(file) = File::open(&path) else {
                continue;
            }; // Read-only.
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() || !(u64::from(RAM_SIZE)..=0x10000000).contains(&meta.len()) {
                continue;
            }
            return Some(Self {
                file,
                path,
                device: meta.dev(),
                inode: meta.ino(),
            });
        }
        None
    }
    fn connected(&self) -> bool {
        // Keeping an opened deleted shm file alive must not keep an old game alive.
        fs::metadata(&self.path).is_ok_and(|m| m.dev() == self.device && m.ino() == self.inode)
    }
}
#[derive(Default)]
pub(super) struct Reader {
    ram: Option<Ram>,
    retry: Option<Instant>,
}
impl Reader {
    pub(super) fn poll(&mut self, session: &Session, game: Option<u32>) -> Option<Snapshot> {
        if game != Some(GAME) || process_start(session.pid).as_deref() != Some(&session.start) {
            self.ram = None;
            self.retry = None;
            return None;
        }
        if self.ram.as_ref().is_some_and(|ram| !ram.connected()) {
            self.ram = None;
            self.retry = None;
        }
        let now = Instant::now();
        if self.ram.is_none() && self.retry.is_none_or(|retry| now >= retry) {
            self.ram = Ram::discover(Path::new("/proc"), session.pid);
            self.retry = Some(now + Duration::from_secs(2));
        }
        let mut snapshot = decode(&self.ram.as_ref()?.file)?;
        snapshot.select_ceiling();
        Some(snapshot)
    }
}

/// Historical measurements for the diagnostic CLI, never a gauge override.
fn measured_speed(config: &str, key: &str) -> Option<f32> {
    config
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('#').next()?.split_whitespace();
            if fields.next()? != key {
                return None;
            }
            let value: f32 = fields.next()?.parse().ok()?;
            (fields.next().is_none() && value.is_finite() && (100.0..=10000.0).contains(&value))
                .then_some(value)
        })
        .next_back()
}

/// Estimated normal full-throttle speed, independent of current speed/boosts.
/// Propulsion equilibrium comes from the GX physics routine; forward resistance
/// has two fitted mass coefficients (2026-10-09). Eight training setups plus a
/// held-out Fat Shark test were within 1.5 km/h of measured/curve references.
/// The ordinary coefficient domain is deliberately bounded; this is an estimate,
/// not a decoded universal law for arbitrary altered machine physics.
fn estimated_speed(stats: [f32; 4]) -> Option<f32> {
    let [mass, acceleration, top, drag] = stats;
    if !(100.0..=10000.0).contains(&mass)
        || !(0.1..=2.0).contains(&acceleration)
        || !(0.001..=2.0).contains(&top)
        || !(0.0001..=0.05).contains(&drag)
    {
        return None;
    }
    let [mass, acceleration, top, drag] = stats.map(f64::from);
    let q = 40.0 * acceleration / 348.0;
    let k = (q - drag) * (36.0 + 40.0 * top) / (4.0 * acceleration * (acceleration + 0.6));
    let resistance = 0.007960438333413056 + 0.6947665111496829 / mass;
    if k <= 0.0 {
        return None;
    }
    let equilibrium = |v: f64| {
        let delta = resistance * v * v;
        delta * (v + delta)
    };
    let (mut low, mut high) = (100.0 / 216.0, 10000.0 / 216.0);
    if equilibrium(low) > k || equilibrium(high) < k {
        return None;
    }
    for _ in 0..48 {
        let v = (low + high) / 2.0;
        if equilibrium(v) > k {
            high = v;
        } else {
            low = v;
        }
    }
    Some(((low + high) / 2.0 * 216.0) as f32)
}

pub(super) fn command(args: &[&str]) -> Result<String, String> {
    let chosen = match args {
        [] => None,
        [speed] => {
            let value = speed
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite() && (100.0..=10000.0).contains(v))
                .ok_or("measured cruising speed must be 100–10000 km/h")?;
            Some(value)
        }
        _ => return Err("usage: g13map ra fzero [MEASURED_KMH]".into()),
    };
    let session = super::native_session(Path::new("/proc"), super::Source::Dolphin)
        .ok_or("no live Dolphin session with achievement logging")?;
    let snapshot = Reader::default()
        .poll(&session, Some(GAME))
        .ok_or("no validated USA F-Zero GX race data")?;
    let key = snapshot
        .setup
        .as_deref()
        .ok_or("enter a race to inspect its machine/setup")?;
    let path = crate::config_dir().join("fzero-speeds.conf");
    if let Some(value) = chosen {
        use std::io::{Read, Write};
        let mut previous = String::new();
        match File::open(&path) {
            Ok(file) => {
                file.take(16385)
                    .read_to_string(&mut previous)
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        if previous.len() > 16384 {
            return Err("speed measurement file exceeds 16 KiB".into());
        }
        let mut content = previous
            .lines()
            .filter(|line| line.split_whitespace().next() != Some(key))
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("{key} {value}\n"));
        if content.len() > 16384 {
            return Err("speed measurement file exceeds 16 KiB".into());
        }
        fs::create_dir_all(crate::config_dir()).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        let result = file
            .write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .and_then(|_| fs::rename(&tmp, &path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(|e| e.to_string())?;
        return Ok(format!("Recorded {value} km/h measured for this machine/setup; the gauge uses the model estimate."));
    }
    let recorded = File::open(&path).ok().and_then(|file| {
        use std::io::Read;
        let mut bytes = String::new();
        file.take(16385).read_to_string(&mut bytes).ok()?;
        (bytes.len() <= 16384)
            .then(|| measured_speed(&bytes, key))
            .flatten()
    });
    Ok(format!("{}: {:.1} km/h, energy {:.1}%, boost {}; normal cruising speed {}.\nRecorded measurement: {}.\nSetup key: {key}",
        PILOTS[snapshot.pilot as usize], snapshot.speed.unwrap_or(0.0),
        snapshot.energy.unwrap_or(0.0), if snapshot.boosting { "active" } else { "off" },
        snapshot.ceiling.map_or("unavailable".into(), |v| format!("{v:.1} km/h (model estimate)")),
        recorded.map_or("none".into(), |v| format!("{v} km/h"))))
}

/// Clockwise sector from twelve o'clock, with a segmented futuristic outer ring.
fn gauge(bm: &mut Bitmap, fraction: Option<f32>, boost: bool) {
    let fill = fraction.unwrap_or(0.0).clamp(0.0, 1.0);
    for y in -20i32..=20 {
        for x in -20i32..=20 {
            let r2 = x * x + y * y;
            let angle = (x as f32)
                .atan2(-y as f32)
                .rem_euclid(std::f32::consts::TAU);
            if (r2 <= 16 * 16 && fill > 0.0 && angle <= fill * std::f32::consts::TAU)
                || ((18 * 18..=20 * 20).contains(&r2)
                    && angle.rem_euclid(std::f32::consts::TAU / 12.0) > 0.10)
            {
                bm.plot(21 + x, 21 + y);
            }
        }
    }
    if boost && fraction.is_some_and(|f| f > 1.02) {
        // Paired split-tip arrowheads, selected from the offline samples (option E).
        // Black outlines preserve their spacing against the filled disc.
        let chevron = [
            "           #o#           ",
            "          ##o##          ",
            "         ###o###         ",
            "        #### ####        ",
            "       ####   ####       ",
            "      ####     ####      ",
            "     ####       ####     ",
            "    #o##         ##o#    ",
            "   ####           ####   ",
            "  ####             ####  ",
            " ####               #### ",
            "####                 ####",
            " ###                 ### ",
        ];
        for top in [9, 20] {
            for (y, row) in chevron.iter().enumerate() {
                for (x, pixel) in row.bytes().enumerate() {
                    if pixel == b'#' {
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                bm.put(9 + x as i32 + dx, top + y as i32 + dy, false);
                            }
                        }
                    }
                }
            }
        }
        for top in [9, 20] {
            bm.sprite(9, top, &chevron);
        }
    } else if fraction.is_none() {
        bm.halo(13, 18, 17, 7);
        text(bm, 13, 18, "---", 1);
    }
}

impl Snapshot {
    fn select_ceiling(&mut self) {
        self.ceiling = self.stats.and_then(estimated_speed);
    }

    pub(super) fn frame(&self, model: &Model) -> Bitmap {
        if let Some(speed) = self.speed {
            let mut bm = Bitmap::blank();
            gauge(
                &mut bm,
                self.ceiling.map(|ceiling| speed / ceiling),
                self.boosting,
            );
            text(&mut bm, 46, 1, "F-ZERO GX", 1);
            if let Some(ceiling) = self.ceiling {
                let label = format!("{}", ceiling.round() as u32);
                text(&mut bm, 158 - label.len() as i32 * 6, 1, &label, 1);
            }
            let energy = self.energy.unwrap_or(0.0).round() as u32;
            text(&mut bm, 46, 10, &format!("E{energy}%"), 1);
            bm.line(82, 9, 157, 9);
            bm.line(82, 17, 157, 17);
            bm.line(82, 9, 82, 17);
            bm.line(157, 9, 157, 17);
            let width = energy.min(100) * 72 / 100;
            for y in 11..=15 {
                for x in 84..84 + width as i32 {
                    bm.plot(x, y);
                }
            }
            let digits = format!("{}", speed.round() as u32);
            text(
                &mut bm,
                46,
                20,
                &digits,
                if digits.len() <= 4 { 2 } else { 1 },
            );
            text(&mut bm, 112, 26, "KM/H", 1);
            if let (Some(n), Some(t)) = (model.earned, model.total.filter(|t| *t > 0)) {
                for x in 46..=157 {
                    bm.plot(x, 33);
                }
                for x in 46..46 + (u64::from(n.min(t)) * 112 / u64::from(t)) as i32 {
                    bm.plot(x, 34);
                }
            }
            let count = match (model.earned, model.total) {
                (Some(n), Some(t)) => format!("{n}/{t}"),
                (_, Some(t)) => format!("--/{t}"),
                _ => "--/--".into(),
            };
            let mode = match model.hardcore {
                Some(true) => "HC",
                Some(false) => "SC",
                None => "?",
            };
            text(
                &mut bm,
                46,
                36,
                &format!("{count} {mode} +{}", model.gained),
                1,
            );
            return bm;
        }
        let mut bm = Bitmap::blank();
        text(&mut bm, 0, 1, "F-ZERO GX", 1);
        text(
            &mut bm,
            108,
            1,
            match self.phase {
                Phase::Ready => "READY",
                Phase::Racing => "RACING",
                Phase::Finished => "FINISHED",
                Phase::Retired => "RETIRED",
            },
            1,
        );
        let context = match self.mode {
            1 => format!(
                "GP {} CUP{} R{}",
                ["NOVICE", "STANDARD", "EXPERT", "MASTER"][self.difficulty as usize],
                self.cup,
                self.race + 1
            ),
            2 => "TIME ATTACK".into(),
            3 => "PRACTICE".into(),
            _ => format!(
                "STORY {} CH{}",
                ["NORMAL", "HARD", "VERY HARD"][self.difficulty as usize],
                self.chapter + 1
            ),
        };
        text(&mut bm, 0, 10, &context, 1);
        text(&mut bm, 0, 18, PILOTS[self.pilot as usize], 1);
        // Compact RA progress remains visible; unlock cards still supersede this frame.
        progress_bar(&mut bm, model, 32, 2);
        let count = match (model.earned, model.total) {
            (Some(n), Some(t)) if t > 0 => {
                format!("{n}/{t} {}%", u64::from(n) * 100 / u64::from(t))
            }
            (_, Some(t)) => format!("--/{t}"),
            _ => "--/--".into(),
        };
        let mode = match model.hardcore {
            Some(true) => "HC",
            Some(false) => "SC",
            None => "?",
        };
        text(
            &mut bm,
            0,
            36,
            &format!("{count} {mode} SESSION+{}", model.gained),
            1,
        );
        bm
    }
}
// Pilot ordering is documented by the consecutive Master Cup movie CodeNotes.
const PILOTS: [&str; 41] = [
    "MIGHTY GAZELLE",
    "JODY SUMMER",
    "DR. STEWART",
    "BABA",
    "SAMURAI GOROH",
    "PICO",
    "CAPTAIN FALCON",
    "OCTOMAN",
    "MR. EAD",
    "JAMES MCCLOUD",
    "BILLY",
    "KATE ALEN",
    "ZODA",
    "JACK LEVIN",
    "BIO REX",
    "THE SKULL",
    "ANTONIO GUSTER",
    "BEASTMAN",
    "LEON",
    "SUPER ARROW",
    "MRS. ARROW",
    "GOMAR & SHIOH",
    "SILVER NEELSEN",
    "MICHAEL CHAIN",
    "BLOOD FALCON",
    "JOHN TANAKA",
    "DRAQ",
    "ROGER BUSTER",
    "DR. CLASH",
    "BLACK SHADOW",
    "DEATHBORN",
    "DON GENIE",
    "DIGI-BOY",
    "DAI SAN GEN",
    "SPADE",
    "DAIGOROH",
    "PRINCIA",
    "LILY",
    "PJ",
    "QQQ",
    "PHOENIX",
];

#[cfg(test)]
mod tests {
    use super::super::{Display, Source};
    use super::*;
    use std::os::unix::fs::FileExt;

    fn fixture() -> (crate::test_support::Sandbox, File, PathBuf) {
        let sandbox = crate::test_support::Sandbox::new("ra-fzero");
        fs::create_dir_all(crate::state_dir()).unwrap();
        let path = crate::state_dir().join("ram");
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(u64::from(RAM_SIZE)).unwrap();
        populate(&file, 0x801ba960);
        (sandbox, file, path)
    }
    fn populate(file: &File, root: u32) {
        file.write_all_at(b"GFZE01\0\0", 0).unwrap();
        file.write_all_at(&root.to_be_bytes(), ROOT).unwrap();
        for (offset, bytes) in [
            (MODE, vec![1]),
            (FLAGS, vec![0x90, 0x40]),
            (0x802453e9, vec![0]),
            (0x801bddc4, vec![16]),
            (0x801bdd16, vec![1]),
            (0x801bdd17, vec![1]),
            (0x801ba0dc, vec![0]),
            (0x227878, 0x80c465e0u32.to_be_bytes().to_vec()),
        ] {
            file.write_all_at(&bytes, address(root, offset, bytes.len()).unwrap())
                .unwrap();
        }
        let block = 0xc465e0;
        for (offset, bytes) in [
            (6, 16u16.to_be_bytes().to_vec()),
            (8, 2060f32.to_be_bytes().to_vec()),
            (0x17c, 992.5f32.to_be_bytes().to_vec()),
            (0x184, 75f32.to_be_bytes().to_vec()),
            (0x5da, 100u16.to_be_bytes().to_vec()),
            (0x220, 0.616f32.to_be_bytes().to_vec()),
            (0x22c, 0.16336f32.to_be_bytes().to_vec()),
            (0x23c, 0.01f32.to_be_bytes().to_vec()),
        ] {
            file.write_all_at(&bytes, block + offset).unwrap();
        }
    }
    #[test]
    fn relocation_big_endian_and_result_speed_suppression() {
        let (_sandbox, file, _) = fixture();
        let s = decode(&file).unwrap();
        assert_eq!(
            (s.phase, s.speed, s.pilot, s.cup, s.race),
            (Phase::Racing, Some(992.5), 16, 1, 1)
        );
        // A relocated RAM structure must not use the obsolete fixed addresses.
        populate(&file, 0x801bd960);
        let flags = address(0x801bd960, FLAGS, 2).unwrap();
        file.write_all_at(&[0x81, 0x50], flags).unwrap();
        let s = decode(&file).unwrap();
        assert_eq!((s.phase, s.speed), (Phase::Finished, None));
        file.write_all_at(&[0x80, 0x60], flags).unwrap();
        assert_eq!(decode(&file).unwrap().phase, Phase::Retired);
    }
    #[test]
    fn invalid_identity_pointer_values_and_menu_fall_back() {
        let (_sandbox, file, _) = fixture();
        let mode = address(0x801ba960, MODE, 1).unwrap();
        file.write_all_at(&[0xff], mode).unwrap();
        assert!(decode(&file).is_none());
        file.write_all_at(&[7], mode).unwrap();
        assert!(decode(&file).is_none()); // Replay is not a current race.
        populate(&file, 0x801ba960);
        let speed = 0xc465e0 + 0x17c;
        for invalid in [f32::NAN, f32::INFINITY, -1.0, 100001.0] {
            file.write_all_at(&invalid.to_be_bytes(), speed).unwrap();
            assert!(decode(&file).is_none());
        }
        populate(&file, 0x801ba960);
        file.write_all_at(&[255], address(0x801ba960, 0x801bddc4, 1).unwrap())
            .unwrap();
        assert!(decode(&file).is_none());
        populate(&file, 0x801ba960);
        file.write_all_at(b"GLME01\0\0", 0).unwrap();
        assert!(decode(&file).is_none());
        // A newer disc revision is attempted, not refused; the populated layout still decodes.
        file.write_all_at(b"GFZE01\0\x01", 0).unwrap();
        assert!(decode(&file).is_some());
        populate(&file, 0x801ba960);
        for root in [0u32, 0x801ba961, 0x81800000, 0x817ffffc] {
            file.write_all_at(&root.to_be_bytes(), ROOT).unwrap();
            assert!(decode(&file).is_none());
        }
        assert_eq!(
            address(0x80000000, RAM_SIZE - 4, 4),
            Some(u64::from(RAM_SIZE - 4))
        );
        assert_eq!(address(0x80000000, RAM_SIZE - 4, 5), None);
    }
    #[test]
    fn player_energy_boost_and_calibration_are_bound_to_setup() {
        let (_sandbox, file, _) = fixture();
        let s = decode(&file).unwrap();
        assert_eq!(s.energy, Some(75.0));
        assert!(!s.boosting);
        let key = s.setup.unwrap();
        assert_eq!(
            measured_speed(&format!("{key} 990 # measured\n"), &key),
            Some(990.0)
        );
        for value in ["NaN", "inf", "0", "99", "10001", "1000 junk"] {
            assert_eq!(measured_speed(&format!("{key} {value}"), &key), None);
        }
        file.write_all_at(&[60], 0xc465e0 + 0x18a).unwrap();
        assert!(decode(&file).unwrap().boosting);
        file.write_all_at(&0.62f32.to_be_bytes(), 0xc465e0 + 0x220)
            .unwrap();
        assert_ne!(decode(&file).unwrap().setup.as_deref(), Some(key.as_str()));
        file.write_all_at(&101f32.to_be_bytes(), 0xc465e0 + 0x184)
            .unwrap();
        assert!(decode(&file).is_none());
        populate(&file, 0x801ba960);
        file.write_all_at(&1u16.to_be_bytes(), 0xc465e0 + 4)
            .unwrap();
        assert!(decode(&file).is_none());
        populate(&file, 0x801ba960);
        file.write_all_at(
            &0u32.to_be_bytes(),
            address(0x801ba960, 0x227878, 4).unwrap(),
        )
        .unwrap();
        assert!(decode(&file).is_none());
    }
    #[test]
    fn estimated_reference_matches_measured_setups_and_held_out_machine() {
        // Independently measured/curve-extrapolated references, not model outputs.
        for (stats, reference) in [
            ([1260.0, 0.735, 0.092125, 0.01], 952.0),
            ([1260.0, 0.45, 0.1, 0.01], 1002.0),
            ([1260.0, 0.315, 0.172, 0.01], 1034.0),
            ([2060.0, 0.7, 0.14991875, 0.01], 984.0),
            ([2060.0, 0.7, 0.17, 0.01], 990.0),
            ([2060.0, 0.49, 0.1534, 0.01], 1022.0),
            ([780.0, 0.6, 0.2, 0.01], 995.0),
            ([2080.0, 0.39, 0.35, 0.015], 1040.0),
            // Fat Shark was held out: coefficients were frozen before this run.
            ([2490.0, 0.52, 0.4, 0.01], 1091.2138),
            // Later validation runs; none changed the model coefficients.
            ([1960.0, 0.264, 0.28056, 0.01], 1069.0163),
            ([1080.0, 0.5964, 0.131495, 0.01], 984.9192),
            ([2340.0, 0.6248, 0.153935, 0.01], 999.975),
            ([1414.0, 0.517, 0.13, 0.01], 1003.7546),
        ] {
            let predicted = estimated_speed(stats).unwrap();
            assert!(
                (predicted - reference).abs() < 1.5,
                "{stats:?}: {predicted} vs {reference}"
            );
        }
    }
    #[test]
    fn recorded_measurements_do_not_override_any_setup() {
        let (_sandbox, file, _) = fixture();
        let mut snapshot = decode(&file).unwrap();
        let key = snapshot.setup.clone().unwrap();
        let path = crate::config_dir().join("fzero-speeds.conf");
        fs::create_dir_all(crate::config_dir()).unwrap();
        let record = format!("{key} 990\n");
        fs::write(&path, &record).unwrap();
        assert_eq!(measured_speed(&record, &key), Some(990.0));
        snapshot.select_ceiling();
        let predicted = snapshot.ceiling.unwrap();
        assert!((predicted - 1003.0).abs() < 1.5);
        assert_ne!(predicted, 990.0);
        // Changing setup coefficients computes a fresh reference, even with old records.
        snapshot.stats = Some([2490.0, 0.52, 0.4, 0.01]);
        snapshot.select_ceiling();
        assert!((snapshot.ceiling.unwrap() - 1092.63).abs() < 0.01);
        snapshot.stats = Some([30000.0, 0.616, 0.16336, 0.01]);
        snapshot.select_ceiling();
        assert_eq!(snapshot.ceiling, None);
        assert_eq!(fs::read_to_string(&path).unwrap(), record);
    }
    #[test]
    fn estimation_rejects_invalid_or_unsupported_physics() {
        let ordinary = [1260.0, 0.45, 0.1, 0.01];
        for index in 0..4 {
            for bad in [f32::NAN, f32::INFINITY, -1.0, 0.0, 100001.0] {
                let mut stats = ordinary;
                stats[index] = bad;
                assert_eq!(estimated_speed(stats), None);
            }
        }
        // Drag exceeds propulsion: no positive cruising equilibrium.
        assert_eq!(estimated_speed([1260.0, 0.1, 0.1, 0.05]), None);
    }
    #[test]
    fn pie_is_clockwise_and_arrow_requires_boost_above_102_percent() {
        let mut empty = Bitmap::blank();
        gauge(&mut empty, Some(0.0), false);
        let mut quarter = Bitmap::blank();
        gauge(&mut quarter, Some(0.25), false);
        assert!(!empty.get(25, 15));
        assert!(quarter.get(25, 15));
        assert!(!quarter.get(17, 15));
        assert!(!quarter.get(25, 27));
        let mut full = Bitmap::blank();
        gauge(&mut full, Some(1.0), false);
        for (fraction, boost) in [(1.02, true), (1.5, false)] {
            let mut frame = Bitmap::blank();
            gauge(&mut frame, Some(fraction), boost);
            assert_eq!(frame, full);
        }
        let mut boost = Bitmap::blank();
        gauge(&mut boost, Some(1.0201), true);
        assert_ne!(boost, full);
    }
    #[test]
    #[ignore = "exports synthetic LCD previews only when explicitly requested"]
    fn render_samples() {
        let (_sandbox, file, _) = fixture();
        let dir =
            PathBuf::from(std::env::var_os("G13MAP_FZERO_DUMP").expect("set output directory"));
        fs::create_dir_all(&dir).unwrap();
        let model = Model {
            game: Some(GAME),
            earned: Some(23),
            total: Some(229),
            hardcore: Some(true),
            gained: 1,
            ..Model::default()
        };
        for (name, speed, boost) in [
            ("empty", 0.0, false),
            ("half", 500.0, false),
            ("full", 1000.0, false),
            ("boost", 1300.0, true),
        ] {
            let mut snapshot = decode(&file).unwrap();
            snapshot.speed = Some(speed);
            snapshot.ceiling = Some(1000.0); // Preview scale, never a runtime default.
            snapshot.boosting = boost;
            save_frame(&dir, name, &snapshot.frame(&model));
        }
        let mut snapshot = decode(&file).unwrap();
        snapshot.select_ceiling();
        save_frame(&dir, "estimated", &snapshot.frame(&model));
    }
    #[test]
    fn replaced_descriptor_cannot_hold_an_old_game_alive() {
        let (_sandbox, file, path) = fixture();
        let meta = file.metadata().unwrap();
        let ram = Ram {
            file,
            path: path.clone(),
            device: meta.dev(),
            inode: meta.ino(),
        };
        assert!(ram.connected());
        fs::rename(&path, path.with_extension("old")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(!ram.connected());
    }
    #[test]
    fn dispatch_fallback_and_trophy_priority() {
        let (_sandbox, file, _) = fixture();
        let snapshot = decode(&file).unwrap();
        let now = Instant::now();
        let mut model = Model {
            game: Some(GAME),
            title: "F-Zero GX".into(),
            earned: Some(0),
            total: Some(229),
            hardcore: Some(true),
            ..Model::default()
        };
        let mut display = Display::default();
        let generic = display.frame_for(&mut model, now, Source::Dolphin);
        let specific = display.frame_with(&mut model, now, Source::Dolphin, Some(&snapshot));
        assert_ne!(specific, generic);
        assert_eq!(
            display.frame_with(&mut model, now, Source::Dolphin, None),
            generic
        );
        model.game = Some(15543);
        assert_eq!(
            display.frame_with(&mut model, now, Source::Dolphin, Some(&snapshot)),
            display.frame_for(&mut model, now, Source::Dolphin)
        );
        model.game = Some(GAME);
        model.queue.push_back("A real unlock".into());
        let trophy = display.frame_with(&mut model, now, Source::Dolphin, Some(&snapshot));
        assert_eq!(trophy, display.frame_for(&mut model, now, Source::Dolphin));
        assert_ne!(trophy, specific);
        assert_eq!(
            display.frame_with(
                &mut model,
                now + Duration::from_secs(8),
                Source::Dolphin,
                Some(&snapshot)
            ),
            specific
        );
    }
    #[test]
    #[ignore = "reads the live F-Zero shared RAM and writes a frame only when explicitly requested"]
    fn live_fzero_ram() {
        let dir =
            PathBuf::from(std::env::var_os("G13MAP_FZERO_DUMP").expect("set G13MAP_FZERO_DUMP"));
        let session = super::super::native_session(Path::new("/proc"), Source::Dolphin)
            .expect("one live Dolphin");
        let snapshot = Reader::default()
            .poll(&session, Some(GAME))
            .expect("valid F-Zero RAM");
        println!("{snapshot:?}");
        let model = Model {
            game: Some(GAME),
            earned: Some(0),
            total: Some(229),
            hardcore: Some(true),
            ..Model::default()
        };
        fs::create_dir_all(&dir).unwrap();
        let bm = snapshot.frame(&model);
        save_frame(&dir, "live", &bm);
    }
    fn save_frame(dir: &Path, name: &str, bm: &Bitmap) {
        fs::write(dir.join(format!("{name}.lpbm")), &bm.0).unwrap();
        let mut bytes = b"P4\n160 43\n".to_vec();
        for y in 0..43 {
            for col in 0..20 {
                let mut b = 0;
                for bit in 0..8 {
                    if !bm.get(col * 8 + bit, y) {
                        b |= 0x80 >> bit;
                    }
                }
                bytes.push(b);
            }
        }
        fs::write(dir.join(format!("{name}.pbm")), bytes).unwrap();
    }
}
