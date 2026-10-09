// SPDX-License-Identifier: GPL-3.0-or-later
// Read-only investigation helper. Never attaches, stops, or writes to the target.
use std::{fs::{self, File}, os::unix::fs::FileExt};
fn main() -> std::io::Result<()> {
 let a: Vec<String> = std::env::args().collect();
 if a.get(1).map(String::as_str)==Some("--version") {println!("factorio-probe 0.1.0");return Ok(())}
 let pid=&a[1]; let maps=fs::read_to_string(format!("/proc/{pid}/maps"))?;
 let base=maps.lines().find(|l|l.contains("/factorio")&&l.split_whitespace().nth(2)==Some("00000000")).unwrap().split('-').next().unwrap();
 let base=u64::from_str_radix(base,16).unwrap();
 let m=File::open(format!("/proc/{pid}/mem"))?;
 let read=|addr:u64|->std::io::Result<u64>{let mut b=[0;8];m.read_exact_at(&mut b,addr)?;Ok(u64::from_le_bytes(b))};
 let mut addr=if a[2]=="global"{read(base+0x4255870)?}else{u64::from_str_radix(a[2].trim_start_matches("0x"),16).unwrap()};
 for s in a.iter().skip(3).take(a.len()-4) {addr=read(addr+u64::from_str_radix(s,16).unwrap())?;}
 println!("base={base:x} addr={addr:x}");
 let len=usize::from_str_radix(a.last().unwrap(),16).unwrap().min(0x4000);
 let mut b=vec![0;len];m.read_exact_at(&mut b,addr)?;
 for (i,v) in b.chunks_exact(8).enumerate(){let u=u64::from_le_bytes(v.try_into().unwrap());println!("{:04x} {:016x} rel={:x} f32={:?} f64={:?}",i*8,u,u.wrapping_sub(base),[f32::from_le_bytes(v[..4].try_into().unwrap()),f32::from_le_bytes(v[4..].try_into().unwrap())],f64::from_le_bytes(v.try_into().unwrap()));}
 Ok(())
}
