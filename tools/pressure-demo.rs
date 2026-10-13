// SPDX-License-Identifier: GPL-3.0-or-later
//! Hardware-free preview of the same System pressure page used by the watcher.
#[path = "../src/pressure/data.rs"]
mod data;
use std::{
    env, fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
const VERSION: &str = "g13-pressure-demo 0.2.0";
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("{VERSION}");
        return Ok(());
    }
    if args.is_empty() || args.iter().any(|a| a == "--help") {
        println!("{VERSION}\nUsage: g13-pressure-demo --preview FILE.pgm [--seconds 1..3600] [--synthetic]\nHardware-free preview. Select System pressure beside Health mode in the editor for the pad.\nThe former --live LCD ownership path is retired: it blocked editor opening.");
        return Ok(());
    }
    let mut output = None;
    let mut seconds = 1u64;
    let mut synthetic = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preview" => output = Some(PathBuf::from(it.next().ok_or("missing preview path")?)),
            "--seconds" => seconds = it.next().ok_or("missing seconds")?.parse()?,
            "--synthetic" => synthetic = true,
            "--live" => return Err("Select System pressure beside Health mode in the editor; the blocking standalone hardware demo is retired".into()),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if !(1..=3600).contains(&seconds) {
        return Err("seconds must be 1..3600".into());
    }
    let output = output.ok_or("choose --preview FILE.pgm")?;
    let started = Instant::now();
    let mut sampler = data::Sampler::new("/proc/pressure".into(), started);
    loop {
        sampler.sample(Instant::now());
        let frame = if synthetic {
            data::example_frame()
        } else {
            sampler.frame()
        };
        let mut bytes = b"P5\n160 43\n255\n".to_vec();
        for y in 0..43 {
            for x in 0..160 {
                bytes.push(if frame[x + y / 8 * 160] & (1 << (y % 8)) != 0 {
                    215
                } else {
                    28
                });
            }
        }
        fs::write(&output, bytes)?;
        if started.elapsed() + Duration::from_secs(1) >= Duration::from_secs(seconds) {
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
