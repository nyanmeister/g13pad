// SPDX-License-Identifier: GPL-3.0-or-later
//! The refactor oracle (2026-11-07): dumps what the library does with the owner's real
//! configuration and a corpus of feed lines, as text, into `$G13MAP_ORACLE_OUT`. Run before
//! and after a change and diff the directories. Ignored: it needs the fixture env
//! (`G13MAP_CONFIG`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` pointing at scratch copies).
//! Nothing here touches the pad or the live configuration.
use std::{
    collections::hash_map::DefaultHasher,
    fmt::Write as _,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("G13MAP_ORACLE_OUT").expect("G13MAP_ORACLE_OUT"))
}

fn put(name: &str, text: &str) {
    let dir = out_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

fn hash(bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn bitmap(b: &crate::lcd::Bitmap) -> String {
    format!("{} lit {}", hash(&b.0), b.lit())
}

const WRITTEN: SystemTime = UNIX_EPOCH;
fn written() -> SystemTime {
    WRITTEN + Duration::from_secs(1_000_000)
}

/// Feed lines per family: the modules' own test literals, the live files of 2026-11-07, and
/// edge cases (expired, zero and duplicate ttl, garbage, version words, off/wait/quiet).
const ALERT: &[&str] = &[
    "alert 10 X ttl 5",
    "alert 225 COM_HQ kind lost sev 5 at 1000000 ttl 8",
    "alert 225 COM kind hit sev 5 ttl 30",
    "alert 350 LOTUS kind hit sev 1 ttl 30",
    "alert 40 FACTORY kind lost sev 3 ttl 30",
    "off",
    "quiet",
    "quiet sev 3",
    "quiet ttl 30",
    "quiet ttl 5",
    "ttl",
    "alert 10 X ttl 0",
    "alert 10 X ttl 5 ttl 6",
    "alert 360 X ttl 5",
    "alert 10 a_very_long_label_here ttl 5",
    "alert 10 X kind bogus ttl 5",
    "alert 10 X sev 9 ttl 5",
    "alert 10 X",
    "alert 10 X ttl 5 extra 1",
    "",
    "   ",
    "alert -1 X ttl 5",
    "alert 10 X at -5 ttl 5",
];
const SHIP: &[&str] = &[
    "ship 1 hull 1000 shield none energy 78 ttl 3",
    "ship 1 hull 20 shield none energy 80 ttl 3",
    "ship 1 hull 56 shield 0 energy 93 ttl 3",
    "ship 1 hull 74 shield 0 ttl 3",
    "ship 1 hull 74 shield 38 energy 78 ttl 0",
    "ship 1 hull 74 shield 38 energy 78 ttl 3",
    "ship 1 hull 999 shield 50 energy 1 repair 3 shield_delay 12 energy_time -4 ttl 3",
    "ship 1 hull 74 hull 75 shield 38 energy 78 ttl 3",
    "ship 2 hull 74 shield 38 energy 78 ttl 3",
    "wait ttl 3",
    "off",
    "ship 1 hull 74 shield 38 energy 78",
    "ship 1 hull -1 shield 38 energy 78 ttl 3",
    "ship 1 hull 74 shield 38 energy 78 ttl 3 bogus 1",
    "",
];
const COMBAT: &[&str] = &[
    "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 3",
    "combat 1 hp 100 shots 0 hits 0 style 0 time 0.000 deaths 0 rank D known 1 ttl 3",
    "combat 1 hp 1000 shots 1 hits 2 style 1 time 1 deaths 0 rank Z known 0 ttl 3",
    "combat 1 hp 0 shots 1 hits 1 style 1 time 1 deaths 1 rank D known 1 ttl 3",
    "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 0",
    "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl inf",
    "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1",
    "combat 1 hp 74 hp 75 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 3",
    "wait ttl 3",
    "off",
    "",
];
const HEALTH: &[&str] = &[
    "65 shield 0 count SKULL 45/0 ttl 9", "65 shield 0 count SKULL x ttl 9",
    "65 shield 0 deaths 3 count SKULL 45/100 count POE 7 ttl 9", "70 ttl 0", "70 ttl 1",
    "70 ttl 1 bogus 7", "70 ttl bogus", "88 shield 0 foe STALFOS 4/8 ttl 9",
    "110.0/110.0 shield 25.0/25.0 ttl 15", "wait", "wait ttl 3", "off", "70", "1200 ttl 5", "70 ttl 5 ttl 6",
    "70 shield 30 helmet 10 mana 5/10 defense 3 breath 4/9 vehicle TANK 50 rank S style 200 time 61 ttl 5",
    "travel 1 ttl 5", "adv 1 ttl 5", "fort 1 ttl 5", "",
];
const AGENT: &[&str] = &[
    "idle ttl 1",
    "off",
    "wait",
    "turn EDITING started 1000 agents 12 shells 1 context_gx 75 ttl 30",
    "turn RUNNING_BASH started 8345 agents 2 last 178900 fail 9600 ttl 30",
    "ready ttl 5",
    "denied ttl 5",
    "ask ATTENTION started 1000 ttl 5",
    "turn thinking ttl 0",
    "turn x ttl 5 ttl 6",
    "bogus ttl 5",
    "",
];

fn dump_profiles() -> String {
    let mut s = String::new();
    let base = crate::daemon_base();
    writeln!(
        s,
        "daemon base: {}",
        base.as_ref()
            .map_or("none".into(), |b| format!("{} binds", b.binds.len()))
    )
    .unwrap();
    for name in crate::profile_names() {
        let (p, notes) = crate::load(&name).unwrap();
        writeln!(
            s,
            "\n## {name}\nnotes: {notes:?}\nneeds_watcher {} health {:?} display {:?}",
            p.needs_watcher(),
            p.health(),
            p.display
        )
        .unwrap();
        writeln!(
            s,
            "-- to_text\n{}-- commands(None,false)\n{}-- commands(base,true)\n{}",
            p.to_text(),
            p.commands(None, false),
            p.commands(base.as_ref(), true)
        )
        .unwrap();
        let again = crate::profile::Profile::parse(&p.to_text());
        writeln!(s, "round trip equal {} notes {:?}", again.0 == p, again.1).unwrap();
    }
    s
}

fn dump_configs() -> String {
    let mut s = String::new();
    let modes = crate::modes::Modes::load();
    writeln!(s, "## modes\n{modes:?}\n{}", modes.to_text()).unwrap();
    let rules = crate::focus::Rules::load();
    writeln!(s, "## focus\n{rules:?}\n{}", rules.to_text()).unwrap();
    let t = crate::meter::Tuning::load();
    writeln!(s, "## meter\n{t:?}\n{}", t.to_text()).unwrap();
    for name in crate::profile_names() {
        let tp = crate::meter::Tuning::load_for(&name);
        if tp != t {
            writeln!(s, "## meter.d/{name}\n{tp:?}\n{}", tp.to_text()).unwrap();
        }
    }
    let g = crate::glass::Glass::load();
    writeln!(s, "## glass\n{g:?}\n{}", g.to_text()).unwrap();
    for name in crate::lcd::names() {
        let o = crate::text_options::Options::load(&name);
        if o != crate::text_options::Options::default() {
            writeln!(s, "## textopts {name}\n{o:?}\n{}", o.to_text()).unwrap();
        }
        if let Some(o) = crate::lcd::options(&name) {
            writeln!(s, "## lcd options {name}\n{o:?}\n{}", o.to_text()).unwrap();
        }
    }
    let settings = crate::panel::Settings::load();
    writeln!(s, "## panel\n{settings:?}\n{}", settings.to_text()).unwrap();
    s
}

fn dump_feeds() -> String {
    let mut s = String::new();
    let w = written();
    for (label, now) in [
        ("now = written + 1 s", w + Duration::from_secs(1)),
        ("now = written + 100 s", w + Duration::from_secs(100)),
    ] {
        writeln!(s, "\n#### {label}").unwrap();
        for l in ALERT {
            writeln!(s, "alert   {l:?} => {:?}", crate::alert::parse(l, w, now)).unwrap();
        }
        for l in SHIP {
            writeln!(s, "ship    {l:?} => {:?}", crate::ship::parse(l, w, now)).unwrap();
        }
        for l in COMBAT {
            writeln!(s, "combat  {l:?} => {:?}", crate::combat::parse(l, w, now)).unwrap();
        }
        for l in HEALTH {
            writeln!(s, "health  {l:?} => {:?}", crate::meter::parse(l, w, now)).unwrap();
        }
        for l in AGENT {
            writeln!(s, "agent   {l:?} => {:?}", crate::agent::parse(l, w, now)).unwrap();
        }
    }
    s
}

/// The live-display frames for every corpus state (and None), at a fixed clock.
fn dump_frames() -> String {
    let mut s = String::new();
    let w = written();
    let now = w + Duration::from_secs(1);
    writeln!(
        s,
        "alert None => {}",
        bitmap(&crate::alert::frame(None, now))
    )
    .unwrap();
    for l in ALERT {
        if let Some(st) = crate::alert::parse(l, w, now) {
            for dt in [1u64, 7, 70, 700] {
                writeln!(
                    s,
                    "alert {l:?} +{dt}s => {}",
                    bitmap(&crate::alert::frame(Some(&st), w + Duration::from_secs(dt)))
                )
                .unwrap();
            }
        }
    }
    // The meter over every health corpus state, three ticks apart, and its colour.
    let mut meter = crate::meter::Meter::default();
    for l in HEALTH {
        if let Some(st) = crate::meter::parse(l, w, now) {
            for i in 0..3 {
                let f = meter.frame(st);
                writeln!(
                    s,
                    "meter {l:?} tick {i} => {} colour {:?}",
                    bitmap(&f),
                    meter.colour(st, [0, 0, 255])
                )
                .unwrap();
            }
        }
    }
    // The agent screen over its corpus at three ages.
    for l in AGENT {
        if let Some(st) = crate::agent::parse(l, w, now) {
            for age in [0u64, 1500, 61_000] {
                let age = Duration::from_millis(age);
                let f = crate::agent::render::frame(&st, age, w + age);
                writeln!(
                    s,
                    "agent {l:?} age {age:?} => {} colour {:?}",
                    bitmap(&f),
                    crate::agent::render::colour(&st, age, w + age, [0, 0, 255])
                )
                .unwrap();
            }
        }
    }
    // The computer-load screen over a fixed /proc/pressure snapshot, sampled a few times.
    let root = std::env::temp_dir().join(format!("g13pad-oracle-psi-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for (name, avg, total) in [
        ("cpu", "12.34", 1_000_000u64),
        ("memory", "0.00", 10),
        ("io", "77.70", 9_999_999),
    ] {
        fs::write(
            root.join(name),
            format!("some avg10={avg} avg60=0.00 avg300=0.00 total={total}\n"),
        )
        .unwrap();
    }
    let t0 = std::time::Instant::now();
    let mut sampler = crate::pressure::data::Sampler::new(root.clone(), t0);
    for i in 1..=3 {
        sampler.sample(t0 + Duration::from_secs(i));
        writeln!(s, "pressure sample {i} => {}", hash(&sampler.frame())).unwrap();
    }
    let _ = fs::remove_dir_all(&root);
    writeln!(s, "ship None => {}", bitmap(&crate::ship::frame(None))).unwrap();
    for l in SHIP {
        if let Some(st) = crate::ship::parse(l, w, now) {
            writeln!(s, "ship {l:?} => {}", bitmap(&crate::ship::frame(Some(st)))).unwrap();
        }
    }
    writeln!(s, "combat None => {}", bitmap(&crate::combat::frame(None))).unwrap();
    for l in COMBAT {
        if let Some(st) = crate::combat::parse(l, w, now) {
            writeln!(
                s,
                "combat {l:?} => {}",
                bitmap(&crate::combat::frame(Some(st)))
            )
            .unwrap();
        }
    }
    s
}

fn dump_pictures() -> String {
    let mut s = String::new();
    for name in crate::lcd::names() {
        match crate::lcd::load(&name) {
            Ok(b) => writeln!(s, "{name}: {}", bitmap(&b)).unwrap(),
            Err(e) => writeln!(s, "{name}: error {e}").unwrap(),
        }
        if let Some(a) = crate::lcd::load_anim(&name) {
            writeln!(
                s,
                "{name} anim: {} frames, animated {}, {}",
                a.frames.len(),
                a.animated(),
                hash(&a.to_bytes())
            )
            .unwrap();
        }
        writeln!(
            s,
            "{name}: origin {:?} source {:?} users {:?}",
            crate::lcd::origin(&name),
            crate::lcd::source(&name).is_some(),
            crate::lcd::users(&name)
        )
        .unwrap();
    }
    for text in [
        "HELLO",
        "a much longer marquee text that scrolls past the edge of the panel",
        "",
        "ÜNÏCÖDE ✓",
    ] {
        match crate::marquee::render(text) {
            Ok(a) => writeln!(
                s,
                "marquee {text:?}: {} frames {}",
                a.frames.len(),
                hash(&a.to_bytes())
            )
            .unwrap(),
            Err(e) => writeln!(s, "marquee {text:?}: error {e}").unwrap(),
        }
    }
    s
}

/// read() over files: which of two feed files wins, by family. Uses the scratch state and
/// runtime dirs from the environment; every scenario starts from empty dirs.
fn dump_reads() -> String {
    let mut s = String::new();
    let state = crate::state_dir();
    let runtime: PathBuf = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    // A fixed clock so the dumps diff clean: files are "written" in 2023 with a 31-year ttl.
    let base = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let put_at = |p: &Path, text: &str, dt: u64| {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(base + Duration::from_secs(dt))
            .unwrap();
    };
    let clear = || {
        let _ = fs::remove_dir_all(&state);
        let _ = fs::remove_dir_all(&runtime);
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&runtime).unwrap();
    };
    // (family, state file, runtime file, live line, off-ish line)
    let fams: [(&str, PathBuf, PathBuf, &str, &str); 4] = [
        ("alert", state.join("alert"), crate::daemon::runtime_file("alert").unwrap(), "alert 10 X ttl 1000000000", "off"),
        ("ship", state.join("ship"), crate::daemon::runtime_file("ship").unwrap(), "ship 1 hull 74 shield 38 energy 78 ttl 1000000000", "wait ttl 1000000000"),
        ("combat", state.join("combat"), crate::daemon::runtime_file("combat").unwrap(), "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 1000000000", "wait ttl 1000000000"),
        ("health", state.join("health"), crate::daemon::runtime_file("health").unwrap(), "70 ttl 1000000000", "off"),
    ];
    let read = |fam: &str| -> String {
        match fam {
            "alert" => format!("{:?}", crate::alert::read()),
            "ship" => format!("{:?}", crate::ship::read()),
            "combat" => format!("{:?}", crate::combat::read()),
            _ => format!("{:?}", crate::meter::read()),
        }
    };
    for (fam, sp, rp, live, off) in &fams {
        writeln!(s, "\n## {fam}").unwrap();
        let big = format!("{live}{}", " ".repeat(2000));
        let mid = format!("{live}{}", " ".repeat(600));
        let scenarios: [(&str, Vec<(&PathBuf, &str, u64)>); 12] = [
            ("nothing", vec![]),
            ("state live", vec![(sp, live, 10)]),
            ("runtime live", vec![(rp, live, 10)]),
            (
                "runtime newer off, state older live",
                vec![(sp, live, 10), (rp, off, 20)],
            ),
            (
                "state newer off, runtime older live",
                vec![(rp, live, 10), (sp, off, 20)],
            ),
            (
                "runtime newer garbage, state older live",
                vec![(sp, live, 10), (rp, "garbage here", 20)],
            ),
            (
                "runtime newer expired, state older live",
                vec![(sp, live, 10), (rp, "alert 10 X ttl 1", 20)],
            ),
            (
                "equal mtime: state live, runtime off",
                vec![(sp, live, 10), (rp, off, 10)],
            ),
            ("oversize runtime live", vec![(rp, big.as_str(), 10)]),
            (
                "equal mtime: state off, runtime live",
                vec![(sp, off, 10), (rp, live, 10)],
            ),
            (
                "state newer wait, runtime older live",
                vec![(rp, live, 10), (sp, "wait ttl 1000000000", 20)],
            ),
            (
                "runtime 600-byte live line, state older live",
                vec![(sp, live, 10), (rp, mid.as_str(), 20)],
            ),
        ];
        for (label, files) in scenarios {
            clear();
            for (p, text, dt) in files {
                put_at(p, text, dt);
            }
            writeln!(s, "{label} => {}", read(fam)).unwrap();
        }
    }
    clear();
    s
}

#[test]
#[ignore = "refactor oracle: G13MAP_ORACLE_OUT=dir G13MAP_CONFIG=fixture XDG_STATE_HOME=scratch XDG_RUNTIME_DIR=scratch cargo test --offline oracle_dump -- --ignored"]
fn oracle_dump() {
    put("profiles.txt", &dump_profiles());
    put("configs.txt", &dump_configs());
    put("feeds.txt", &dump_feeds());
    put("frames.txt", &dump_frames());
    put("pictures.txt", &dump_pictures());
    put("reads.txt", &dump_reads());
}
