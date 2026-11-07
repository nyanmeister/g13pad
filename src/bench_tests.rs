// SPDX-License-Identifier: GPL-3.0-or-later
//! Micro-benchmarks of the hot paths, as an ignored test printed by `--nocapture` under
//! `--release`: parse+write of the owner's profiles, the feed parsers, a frame of every live
//! screen, a marquee. Self-contained (its own corpus) so the same file runs on an older
//! checkout for a before/after. Not a correctness test; `--release` or the numbers mean nothing.
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn per_op(label: &str, iters: u32, mut f: impl FnMut()) {
    // Warm once, then time.
    f();
    let t0 = Instant::now();
    for _ in 0..iters {
        f();
    }
    let per = t0.elapsed().as_secs_f64() * 1e6 / f64::from(iters);
    println!("{label:<40} {per:9.2} µs/op  ({iters} iters)");
}

#[test]
#[ignore = "micro-bench: G13MAP_CONFIG=fixture cargo test --release --offline bench_hot_paths -- --ignored --nocapture"]
fn bench_hot_paths() {
    let w = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let now = w + Duration::from_secs(1);
    let names = crate::profile_names();
    let texts: Vec<String> = names
        .iter()
        .filter_map(|n| std::fs::read_to_string(crate::profile_path(n)).ok())
        .collect();
    println!("profiles in fixture: {}", texts.len());
    per_op("Profile::parse (all fixture profiles)", 200, || {
        for t in &texts {
            std::hint::black_box(crate::profile::Profile::parse(t));
        }
    });
    let parsed: Vec<_> = texts
        .iter()
        .map(|t| crate::profile::Profile::parse(t).0)
        .collect();
    per_op("Profile::to_text + commands (all)", 200, || {
        for p in &parsed {
            std::hint::black_box(p.to_text());
            std::hint::black_box(p.commands(None, false));
        }
    });
    let health = "70 shield 30 helmet on mana 5/10 defense 3 breath 4/9 vehicle TANK rank S style 50 time 61 deaths 2 count SKULL 45/100 ttl 5";
    per_op("health feed parse", 20_000, || {
        std::hint::black_box(crate::meter::parse(health, w, now));
    });
    let alert = "alert 225 COM_HQ kind lost sev 5 at 1000000 ttl 8";
    per_op("alert feed parse", 20_000, || {
        std::hint::black_box(crate::alert::parse(alert, w, now));
    });
    let combat =
        "combat 1 hp 74 shots 50 hits 43 style 8928 time 124.000 deaths 2 rank SS known 1 ttl 3";
    per_op("combat feed parse", 20_000, || {
        std::hint::black_box(crate::combat::parse(combat, w, now));
    });
    let agent =
        "turn RUNNING_BASH started 8345 agents 2 last 178900 fail 9600 context_gx 75 ttl 30";
    per_op("agent feed parse", 20_000, || {
        std::hint::black_box(crate::agent::parse(agent, w, now));
    });
    let hs = crate::meter::parse(health, w, now).unwrap();
    let mut meter = crate::meter::Meter::default();
    per_op("meter frame (health with extras)", 5_000, || {
        std::hint::black_box(meter.frame(hs));
    });
    let ast = crate::agent::parse(agent, w, now).unwrap();
    per_op("agent frame", 5_000, || {
        std::hint::black_box(crate::agent::render::frame(
            &ast,
            Duration::from_millis(1500),
            now,
        ));
    });
    let al = crate::alert::parse(alert, w, now).unwrap();
    per_op("alert frame", 5_000, || {
        std::hint::black_box(crate::alert::frame(Some(&al), now));
    });
    let cs = crate::combat::parse(combat, w, now).unwrap();
    per_op("combat frame", 5_000, || {
        std::hint::black_box(crate::combat::frame(Some(cs)));
    });
    let ss = crate::ship::parse("ship 1 hull 74 shield 38 energy 78 ttl 3", w, now).unwrap();
    per_op("ship frame", 5_000, || {
        std::hint::black_box(crate::ship::frame(Some(ss)));
    });
    per_op("marquee render (short)", 20, || {
        std::hint::black_box(crate::marquee::render("HELLO WORLD").ok());
    });
    let a = crate::lcd::Bitmap::blank();
    let b = crate::lcd::Bitmap::blank();
    per_op("960-byte frame compare", 1_000_000, || {
        std::hint::black_box(a == b);
    });
    let _ = SystemTime::now();
}
