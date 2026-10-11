// SPDX-License-Identifier: GPL-3.0-or-later
//! BN2 USA: public RA CodeNotes and retail ROM routines 0x4e1a, 0x652e,
//! 0x1142a, 0x11612. See docs/battle-network-2.md for address provenance.
use super::core_memory::read_memory;
#[cfg(test)]
use super::core_memory::{append_config, bytes};
use super::{process_start, progress_bar, text, Bitmap, Model, Session};
use crate::draw::Draw;
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{FileExt, OpenOptionsExt},
    path::Path,
    process::ChildStdin,
    sync::mpsc::Receiver,
    time::{Duration, SystemTime},
};

pub(super) const GAME: u32 = 773;
const CRC: &str = "6d961f82";
const REVISION: &str = "AE2E-rev0-style-v1";
const RETAIL: &str = "USA retail";
const NO_MP: &str = "USA No MP Hard Mode v1.1 (Souzooka)";
const HEADER: &[u8] = &[
    77, 69, 71, 65, 77, 65, 78, 95, 69, 88, 69, 50, 65, 69, 50, 69, 48, 56, 150, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 131, 0, 0,
];
const THRESHOLDS: &[u8] = &[120, 160, 130, 100, 0, 181, 2, 72];
const CATEGORIES: [&str; 4] = ["GUTS", "CUSTOM", "TEAM", "SHIELD"];
const REQUIREMENTS: [u8; 4] = [120, 160, 130, 100];
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Snapshot {
    rom: &'static str,
    category: u8,
    element: u8,
    version: u8,
    upgrade: Option<(u8, u8)>,
    upgrade_ready: bool,
    battles: Option<u32>,
    enabled: bool,
    offer: bool,
    votes: [u32; 4],
}

fn decode(main: &[u8], votes: &[u8], flags: &[u8]) -> Option<Snapshot> {
    if main.len() != 72 || votes.len() != 16 || flags.len() != 1 {
        return None;
    }
    // Main block starts at 02000da0. Reject title/loading and unknown phases.
    if ![8, 12, 24, 28].contains(&main[0x20]) {
        return None;
    }
    let style = main[0x21];
    let category = (style >> 3) & 7;
    let element = style & 7;
    let version = (style >> 6) + 1;
    if version > 3
        || category > 5
        || (category == 0 && style != 0)
        || (category == 5 && style != 0x28)
        || ((1..=4).contains(&category) && !(1..=4).contains(&element))
    {
        return None;
    }
    let enabled = flags[0] & 0x10 != 0;
    // Fresh saves leave this word FF until the first story Style is offered.
    let battles = enabled.then(|| u32::from_le_bytes(main[0x44..0x48].try_into().unwrap()));
    if battles.is_some_and(|n| n > 280) || main[0] > 1 {
        return None;
    }
    let mut totals = [0; 4];
    for (i, v) in totals.iter_mut().enumerate() {
        *v = u32::from_le_bytes(votes[i * 4..i * 4 + 4].try_into().ok()?);
        // Initial votes can accumulate before the story gate; retail compares
        // these as signed words, and resets them when a new Style is offered.
        if *v > i32::MAX as u32 {
            return None;
        }
    }
    let upgrade = if (1..=4).contains(&category) && version < 3 {
        let n = main[1 + (category as usize - 1) * 4 + element as usize - 1];
        let total = REQUIREMENTS[category as usize - 1];
        if n >= total {
            return None;
        }
        Some((n, total))
    } else {
        None
    };
    Some(Snapshot {
        rom: RETAIL,
        category,
        element,
        version,
        upgrade,
        upgrade_ready: main[0] == 1,
        battles,
        enabled,
        offer: flags[0] & 0x20 != 0,
        votes: totals,
    })
}

impl Snapshot {
    fn style(&self) -> String {
        match self.category {
            0 => "NORMAL".into(),
            5 => "HUB".into(),
            c => format!(
                "{} {} V{}",
                ["", "ELEC", "HEAT", "AQUA", "WOOD"][self.element as usize],
                CATEGORIES[c as usize - 1],
                self.version
            ),
        }
    }
    fn leader(&self) -> usize {
        // Retail selector overwrites on equal values: Shield wins the final tie.
        (0..4).max_by_key(|&i| self.votes[i]).unwrap()
    }
    fn upgrade_label(&self) -> String {
        if self.upgrade_ready {
            "UPGRADE READY".into()
        } else if let Some((n, total)) = self.upgrade {
            format!("UPGRADE {n}/{total}")
        } else if self.version == 3 {
            "UPGRADE MAX".into()
        } else {
            "UPGRADE --".into()
        }
    }
    pub(super) fn frame(&self, model: &Model) -> Bitmap {
        let mut bm = Bitmap::blank();
        text(&mut bm, 1, 0, &self.style(), 1);
        let next = if self.offer {
            "NEXT STYLE READY".into()
        } else if let Some(n) = self.battles {
            format!("NEXT {n}/280")
        } else {
            "NEXT STORY LOCK".into()
        };
        text(&mut bm, 1, 9, &next, 1);
        // Thin Style progress bar, distinct from the RA completion bar below.
        bm.line(1, 17, 157, 17);
        if let Some(n) = self.battles {
            let width = n * 156 / 280;
            if width > 0 {
                bm.line(1, 18, width as i32, 18);
            }
        }
        let lead = self.leader();
        let leader = if self.votes[lead] == 0 {
            "LEAD --".into()
        } else {
            format!("LEAD {} {}", CATEGORIES[lead], self.votes[lead])
        };
        text(&mut bm, 1, 20, &leader, 1);
        text(&mut bm, 1, 29, &self.upgrade_label(), 1);
        progress_bar(&mut bm, model, 38, 3);
        bm
    }
}

fn variant(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [18, 219] => Some(RETAIL),
        [0, 0] => Some(NO_MP),
        _ => None,
    }
}

pub(super) static RECIPE: super::core_memory::Recipe = super::core_memory::Recipe {
    file: "bn2.json",
    collect,
};

/// Recognize an owned, uncompressed file without guessing from its filename.
/// This is a bounded layout signature, not a full-ROM authenticity check.
pub(super) fn identify_content(path: &Path) -> bool {
    // O_NONBLOCK prevents a special file masquerading as content from blocking
    // stderr draining; regular-file preads are unaffected.
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() == 8 * 1024 * 1024)
    {
        return false;
    }
    let mut header = [0; 32];
    let mut thresholds = [0; 8];
    let mut patch = [0; 2];
    file.read_exact_at(&mut header, 0xa0).is_ok()
        && file.read_exact_at(&mut thresholds, 0x11498).is_ok()
        && file.read_exact_at(&mut patch, 0x1c712).is_ok()
        && header == HEADER
        && thresholds == THRESHOLDS
        && variant(&patch).is_some()
}

fn collect(
    input: &mut ChildStdin,
    receiver: &Receiver<String>,
) -> Option<Option<serde_json::Value>> {
    let header = read_memory(input, receiver, 0x080000a0, 32)?;
    if header.as_deref() != Some(HEADER) {
        return Some(None);
    }
    let mut blocks = Vec::new();
    let thresholds = read_memory(input, receiver, 0x08011498, 8)?;
    let patch = read_memory(input, receiver, 0x0801c712, 2)?;
    let rom = patch.as_deref().and_then(variant);
    for (address, size) in [(0x02000da0, 72), (0x02003a68, 16), (0x02000005, 1)] {
        let reply = read_memory(input, receiver, address, size)?;
        blocks.push(reply);
    }
    let after = read_memory(input, receiver, 0x080000a0, 32)?;
    let valid = header == after
        && thresholds.as_deref() == Some(THRESHOLDS)
        && rom.is_some()
        && blocks.iter().all(Option::is_some);
    if valid {
        Some(Some(serde_json::json!({"revision":REVISION,"variant":rom,
                    "main":blocks[0],"votes":blocks[1],"flags":blocks[2]})))
    } else {
        Some(None)
    }
}

pub(super) fn poll(session: &Session) -> Option<Snapshot> {
    poll_since(session, None)
}
pub(super) fn poll_since(session: &Session, earliest: Option<SystemTime>) -> Option<Snapshot> {
    if session.since.is_some() || process_start(session.pid).as_deref() != Some(&session.start) {
        return None;
    }
    let file = File::open(session.path.parent()?.join("bn2.json")).ok()?;
    let age = SystemTime::now()
        .duration_since(file.metadata().ok()?.modified().ok()?)
        .ok()?;
    if age > TIMEOUT {
        return None;
    }
    let mut data = Vec::new();
    file.take(4097).read_to_end(&mut data).ok()?;
    if data.len() > 4096 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    if let Some(earliest) = earliest {
        let sampled_at = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_millis(value["sampled_at"].as_u64()?))?;
        if sampled_at < earliest || sampled_at > SystemTime::now() {
            return None;
        }
    }
    if value["pid"].as_u64()? != session.pid as u64
        || value["start"].as_str()? != session.start
        || value["revision"].as_str()? != REVISION
    {
        return None;
    }
    let block = |name: &str| -> Option<Vec<u8>> {
        value[name]
            .as_array()?
            .iter()
            .map(|v| u8::try_from(v.as_u64()?).ok())
            .collect()
    };
    let mut snapshot = decode(&block("main")?, &block("votes")?, &block("flags")?)?;
    snapshot.rom = match value["variant"].as_str()? {
        RETAIL => RETAIL,
        NO_MP => NO_MP,
        _ => return None,
    };
    Some(snapshot)
}

pub(super) fn command(args: &[&str]) -> Result<String, String> {
    if !matches!(args, [] | ["preview", _]) {
        return Err("usage: g13map ra bn2 [launch RETROARCH_ARGS | preview FILE.pgm]".into());
    }
    let session = super::session().ok_or("launch with g13map ra launch, then load BN2 USA")?;
    let snapshot = poll(&session)
        .ok_or("no fresh validated BN2 USA gameplay data; requires mGBA and the G13 launcher")?;
    if let ["preview", path] = args {
        let frame = snapshot.frame(&Model::default());
        let mut out = b"P5\n160 43\n255\n".to_vec();
        out.extend(frame.gray());
        fs::write(path, out).map_err(|e| e.to_string())?;
    }
    let lead = snapshot.leader();
    let leader = if snapshot.votes[lead] == 0 {
        "-- (no votes yet)"
    } else {
        CATEGORIES[lead]
    };
    let crc = if snapshot.rom == NO_MP {
        "e99ce5c5"
    } else {
        CRC
    };
    Ok(format!("BN2 {} (validated {REVISION}; reference ROM CRC32 {crc}), PID {}\n{}\nNext Style: {}/280, enabled={}, offer={}\nVotes: Guts {}, Custom {}, Team {}, Shield {}\nLead: {} (Hub may override)\n{}\nRead-only local memory; no battle/achievement inference", snapshot.rom,session.pid,
        snapshot.style(), snapshot.battles.map_or_else(|| "--".into(), |n| n.to_string()), snapshot.enabled, snapshot.offer,
        snapshot.votes[0],snapshot.votes[1],snapshot.votes[2],snapshot.votes[3], leader,snapshot.upgrade_label()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn direct_content_detection_checks_layout_instead_of_filename() {
        let sandbox = crate::test_support::Sandbox::new("bn2-content");
        let path = sandbox.dir.join("unrelated-name.gba");
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        file.write_all_at(HEADER, 0xa0).unwrap();
        file.write_all_at(THRESHOLDS, 0x11498).unwrap();
        file.write_all_at(&[18, 219], 0x1c712).unwrap();
        assert!(identify_content(&path));
        file.write_all_at(&[0, 0], 0x1c712).unwrap();
        assert!(identify_content(&path));
        file.write_all_at(&[1, 0], 0x1c712).unwrap();
        assert!(!identify_content(&path));
        file.set_len(1).unwrap();
        assert!(!identify_content(&path));
        assert!(!identify_content(&sandbox.dir));
        let fifo = sandbox.dir.join("content-fifo");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        assert!(!identify_content(&fifo));
    }
    #[test]
    fn preserves_append_configs_and_style_resets() {
        assert_eq!(
            append_config(
                &["--appendconfig=a.cfg|b.cfg"],
                Path::new("/private/memory.cfg")
            )
            .unwrap(),
            "a.cfg|b.cfg|/private/memory.cfg"
        );
        assert!(append_config(&["--appendconfig"], Path::new("/private/memory.cfg")).is_err());
        let mut main = vec![0; 72];
        main[0x20] = 8;
        main[0x21] = 0x1a;
        main[0x44..0x48].copy_from_slice(&279u32.to_le_bytes());
        let mut votes = [0; 16];
        for i in [0, 1] {
            votes[i * 4..i * 4 + 4].copy_from_slice(&120u32.to_le_bytes());
        }
        let before = decode(&main, &votes, &[0x10]).unwrap();
        assert_eq!(before.leader(), 1); // Custom wins a Guts/Custom tie.
        main[0x44..0x48].fill(0);
        let reset = decode(&main, &[0; 16], &[0x10]).unwrap();
        assert_eq!(reset.battles, Some(0));
        assert_eq!(reset.votes, [0; 4]);
        assert_ne!(before, reset);
    }
    #[test]
    fn fresh_save_counter_is_unknown_until_story_unlock() {
        let mut main = vec![0; 72];
        main[0x20] = 8;
        main[0x44..0x48].fill(0xff);
        let s = decode(&main, &[0; 16], &[0]).unwrap();
        assert_eq!(s.battles, None);
        assert!(!s.enabled);
        assert!(decode(&main, &[0; 16], &[0x10]).is_none());
    }
    #[test]
    fn a_recently_published_sample_from_before_reload_is_rejected() {
        let sandbox = crate::test_support::Sandbox::new("bn2-epoch");
        let pid = std::process::id();
        let start = process_start(pid).unwrap();
        let session = Session {
            pid,
            start: start.clone(),
            path: sandbox.dir.join("achievements.log"),
            since: None,
        };
        let mut main = vec![0u8; 72];
        main[0x20] = 8;
        let boundary = SystemTime::UNIX_EPOCH
            + Duration::from_millis(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            );
        let old = boundary
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 1000;
        let mut value = serde_json::json!({"pid":pid,"start":start,"revision":REVISION,"variant":RETAIL,
            "sampled_at":old,"main":main,"votes":vec![0u8;16],"flags":[0x10]});
        let path = sandbox.dir.join("bn2.json");
        fs::write(&path, value.to_string()).unwrap();
        assert!(poll_since(&session, Some(boundary)).is_none());
        value["sampled_at"] = serde_json::json!(old + 1000);
        fs::write(&path, value.to_string()).unwrap();
        assert!(poll_since(&session, Some(boundary)).is_some());
    }
    #[test]
    fn snapshot_binding_expiry_and_trophies() {
        let sandbox = crate::test_support::Sandbox::new("bn2");
        let pid = std::process::id();
        let start = process_start(pid).unwrap();
        let path = sandbox.dir.join("achievements.log");
        let session = Session {
            pid,
            start: start.clone(),
            path,
            since: None,
        };
        let mut main = vec![0; 72];
        main[0x20] = 8;
        let zero_votes = vec![0u8; 16];
        let value = serde_json::json!({"pid":pid,"start":start,"revision":REVISION,"variant":RETAIL,"main":main,"votes":zero_votes,"flags":[0x10]});
        let snapshot_path = sandbox.dir.join("bn2.json");
        fs::write(&snapshot_path, value.to_string()).unwrap();
        let snapshot = poll(&session).unwrap();
        let mut wrong = session.clone();
        wrong.start.push('0');
        assert!(poll(&wrong).is_none());
        let file = File::open(&snapshot_path).unwrap();
        file.set_times(
            std::fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(3)),
        )
        .unwrap();
        assert!(poll(&session).is_none());
        let now = std::time::Instant::now();
        let mut model = Model {
            game: Some(GAME),
            ..Model::default()
        };
        let mut display = super::super::Display::default();
        assert_eq!(
            display.frame_telemetry(
                &mut model,
                now,
                super::super::Source::RetroArch,
                None,
                Some(&snapshot)
            ),
            snapshot.frame(&model)
        );
        model.queue.push_back("TEST TROPHY".into());
        let trophy = display.frame_telemetry(
            &mut model,
            now,
            super::super::Source::RetroArch,
            None,
            Some(&snapshot),
        );
        assert_ne!(trophy, snapshot.frame(&model));
        model.game = Some(575);
        assert_eq!(
            display.frame_telemetry(
                &mut model,
                now,
                super::super::Source::RetroArch,
                None,
                Some(&snapshot)
            ),
            display.frame_with(&mut model, now, super::super::Source::RetroArch, None)
        );
    }
    #[test]
    #[ignore = "exports synthetic Style layouts only when explicitly requested"]
    fn render_samples() {
        let dir = PathBuf::from(std::env::var_os("G13MAP_BN2_DUMP").expect("set output directory"));
        fs::create_dir_all(&dir).unwrap();
        let model = Model {
            game: Some(GAME),
            earned: Some(12),
            total: Some(80),
            ..Model::default()
        };
        for (name, style, n, battles, flags) in [
            ("normal", 0, 0, 0, 0),
            ("custom", 0x54, 159, 279, 0x10),
            ("max", 0x99, 0, 42, 0x10),
            ("hub", 0x28, 0, 280, 0x30),
        ] {
            let mut main = vec![0; 72];
            main[0x20] = 8;
            main[0x21] = style;
            if (1..=4).contains(&((style >> 3) & 7)) {
                main[1 + (((style >> 3) & 7) as usize - 1) * 4 + (style & 7) as usize - 1] = n;
            }
            main[0x44..0x48].copy_from_slice(&(battles as u32).to_le_bytes());
            let mut votes = [0; 16];
            votes[4..8].copy_from_slice(&145u32.to_le_bytes());
            let s = decode(&main, &votes, &[flags]).unwrap();
            let mut out = b"P5\n160 43\n255\n".to_vec();
            out.extend(s.frame(&model).gray());
            fs::write(dir.join(format!("{name}.pgm")), out).unwrap();
        }
    }
    #[test]
    fn style_upgrade_thresholds_and_final_ties() {
        let mut main = vec![0; 72];
        main[0x20] = 8;
        for (i, &total) in REQUIREMENTS.iter().enumerate() {
            main[0x21] = ((i as u8 + 1) << 3) | 4 | 0x40;
            main[1 + i * 4 + 3] = total - 1;
            let s = decode(&main, &[0; 16], &[0x10]).unwrap();
            assert_eq!(s.upgrade, Some((total - 1, total)));
            assert_eq!(s.leader(), 3);
            main[1 + i * 4 + 3] = total;
            assert!(decode(&main, &[0; 16], &[0x10]).is_none());
            main[1 + i * 4 + 3] = 0;
        }
        main[0x21] = 0x8a;
        assert_eq!(decode(&main, &[0; 16], &[0x10]).unwrap().upgrade, None);
        main[0x20] = 0xff;
        assert!(decode(&main, &[0; 16], &[0x10]).is_none());
    }
    #[test]
    fn protocol_rejects_wrong_games_errors_and_truncation() {
        assert_eq!(variant(&[18, 219]), Some(RETAIL));
        assert_eq!(variant(&[0, 0]), Some(NO_MP));
        assert_eq!(variant(&[18, 0]), None);
        assert_eq!(
            bytes("READ_CORE_MEMORY 2000005 10", 0x02000005, 1),
            Some(vec![0x10])
        );
        for s in [
            "READ_CORE_MEMORY 2000006 10",
            "READ_CORE_MEMORY 2000005 -1 no memory",
            "READ_CORE_MEMORY 2000005 10 20",
            "READ_CORE_MEMORY 2000005",
        ] {
            assert!(bytes(s, 0x02000005, 1).is_none());
        }
    }
}
