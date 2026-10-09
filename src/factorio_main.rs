// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only native Factorio observer. No debugger attach, injection, console, or mods.
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
    process::{self, Command},
    thread,
    time::{Duration, Instant},
};
const BUILD_ID: &str = "60910b0b4f9cff6de7cf1a5a089334784f7945f8";
const GLOBAL: u64 = 0x4255870;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn bytes<const N: usize>(f: &File, at: u64) -> io::Result<[u8; N]> {
    let mut b = [0; N];
    f.read_exact_at(&mut b, at)?;
    Ok(b)
}
fn build_id(f: &File) -> io::Result<String> {
    let h = bytes::<64>(f, 0)?;
    if &h[..6] != b"\x7fELF\x02\x01" || u16::from_le_bytes(h[18..20].try_into().unwrap()) != 62 {
        return Err(invalid("requires native Linux x86-64 ELF"));
    }
    let ph = u64::from_le_bytes(h[32..40].try_into().unwrap());
    let stride = u16::from_le_bytes(h[54..56].try_into().unwrap()) as u64;
    let count = u16::from_le_bytes(h[56..58].try_into().unwrap());
    if stride != 56 || count > 128 || ph > 1_000_000 {
        return Err(invalid("invalid ELF program headers"));
    }
    for i in 0..count {
        let p = bytes::<56>(f, ph + u64::from(i) * stride)?;
        if u32::from_le_bytes(p[..4].try_into().unwrap()) != 4 {
            continue;
        }
        let offset = u64::from_le_bytes(p[8..16].try_into().unwrap());
        let size = u64::from_le_bytes(p[32..40].try_into().unwrap());
        if size > 65536 {
            return Err(invalid("oversize ELF note"));
        }
        let mut b = vec![0; size as usize];
        f.read_exact_at(&mut b, offset)?;
        let mut n = 0;
        while n + 12 <= b.len() {
            let names = u32::from_le_bytes(b[n..n + 4].try_into().unwrap()) as usize;
            let desc = u32::from_le_bytes(b[n + 4..n + 8].try_into().unwrap()) as usize;
            let kind = u32::from_le_bytes(b[n + 8..n + 12].try_into().unwrap());
            let start = n + 12;
            let data = start + names.div_ceil(4) * 4;
            let end = data + desc;
            if end > b.len() {
                return Err(invalid("truncated ELF note"));
            }
            if kind == 3 && names == 4 && &b[start..start + 4] == b"GNU\0" {
                return Ok(b[data..end].iter().map(|v| format!("{v:02x}")).collect());
            }
            n = data + desc.div_ceil(4) * 4;
        }
    }
    Err(invalid("ELF build ID missing"))
}
struct Reader {
    mem: File,
    base: u64,
}
impl Reader {
    fn open(pid: u32) -> io::Result<Self> {
        let exe = File::open(format!("/proc/{pid}/exe"))?;
        let id = build_id(&exe)?;
        if id != BUILD_ID {
            return Err(invalid(&format!(
                "unsupported Factorio build {id}; tested 2.1.21 build 87673"
            )));
        }
        let ino = exe.metadata()?.ino();
        let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
        let base = maps
            .lines()
            .find_map(|l| {
                let p: Vec<_> = l.split_whitespace().collect();
                if p.len() < 5 || p[2] != "00000000" || p[4].parse::<u64>().ok() != Some(ino) {
                    return None;
                }
                u64::from_str_radix(p[0].split('-').next()?, 16).ok()
            })
            .ok_or_else(|| invalid("executable load mapping missing"))?;
        Ok(Self {
            mem: File::open(format!("/proc/{pid}/mem"))?,
            base,
        })
    }
    fn read<const N: usize>(&self, at: u64) -> io::Result<[u8; N]> {
        if !(0x10000..0x0000_8000_0000_0000).contains(&at) || at.checked_add(N as u64).is_none() {
            return Err(invalid("invalid memory address"));
        }
        bytes(&self.mem, at)
    }
    fn ptr(&self, at: u64) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.read(at)?))
    }
    fn byte(&self, at: u64) -> io::Result<u8> {
        Ok(self.read::<1>(at)?[0])
    }
    fn f32(&self, at: u64) -> io::Result<f64> {
        let v = f32::from_le_bytes(self.read(at)?) as f64;
        number(v)
    }
    fn f64(&self, at: u64) -> io::Result<f64> {
        number(f64::from_le_bytes(self.read(at)?))
    }
    fn quality(&self, id: u8) -> io::Result<f64> {
        let table = self.ptr(self.base + 0x425faf0)?;
        let proto = self.ptr(table + u64::from(id) * 8)?;
        self.f32(proto + 0x370)
    }
    fn label(&self, prototype: u64) -> io::Result<String> {
        let addr = self.ptr(prototype + 8)?;
        let length = self.ptr(prototype + 16)?;
        if length == 0 || length > 256 {
            return Err(invalid("invalid prototype name length"));
        }
        let mut b = vec![0; length as usize];
        self.mem.read_exact_at(&mut b, addr)?;
        let s = std::str::from_utf8(&b).map_err(|_| invalid("invalid prototype name"))?;
        Ok(s.chars()
            .take(32)
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect())
    }
    fn equipment(&self, character: u64) -> io::Result<(f64, f64, Option<f64>)> {
        let inventory = self.ptr(character + 0x398)?;
        let item = self.ptr(inventory + 8)?;
        if item == 0 || self.ptr(item)? != self.base + 0x3f97748 {
            return Ok((0., 0., None));
        }
        let grid = self.ptr(item + 0x58)?;
        if grid == 0 {
            return Ok((0., 0., None));
        }
        let bounds = self.read::<16>(grid + 0x20)?;
        let start = u64::from_le_bytes(bounds[..8].try_into().unwrap());
        let end = u64::from_le_bytes(bounds[8..].try_into().unwrap());
        let count = vector_count(start, end, 8, 1024)?;
        let (mut shield, mut max_shield, mut charge, mut capacity) = (0., 0., 0., 0.);
        for i in 0..count {
            let e = self.ptr(start + i * 8)?;
            let vt = self.ptr(e)?;
            if vt == self.base + 0x3e7fac8 {
                let source = self.ptr(e + 0x30)?;
                charge += self.f64(source + 0x20)?;
                capacity += self.f64(source + 0x28)?;
            } else if vt == self.base + 0x3e80728 {
                shield += self.f32(e + 0x48)?;
                let id = u16::from_le_bytes(self.read(e + 0x20)?);
                let table = self.ptr(self.base + 0x425f310)?;
                let proto = self.ptr(table + u64::from(id) * 8)?;
                max_shield += self.f32(proto + 0x330)? * self.quality(self.byte(e + 0x22)?)?;
            }
        }
        if bounds != self.read::<16>(grid + 0x20)? {
            return Err(invalid("equipment changed during read"));
        }
        Ok((
            shield,
            max_shield,
            if capacity > 0. {
                Some((charge / capacity * 100.).clamp(0., 100.))
            } else {
                None
            },
        ))
    }
    fn attacks(&self, player: u64) -> io::Result<u64> {
        // std::map<SurfaceIndex, vector<Alert>> for entity_under_attack (enum 1).
        let header = player + 0xa0 + 0x48;
        let root = self.ptr(header + 8)?;
        let mut pending = vec![root];
        let mut seen = HashSet::new();
        let mut count = 0;
        while let Some(node) = pending.pop() {
            if node == 0 {
                continue;
            }
            if seen.len() >= 256 || !seen.insert(node) {
                return Err(invalid("changing or oversize alert tree"));
            }
            let b = self.ptr(node + 0x28)?;
            let e = self.ptr(node + 0x30)?;
            count += vector_count(b, e, 0x58, 65536)?;
            pending.push(self.ptr(node + 0x10)?);
            pending.push(self.ptr(node + 0x18)?);
        }
        if root != self.ptr(header + 8)? {
            return Err(invalid("alerts changed during read"));
        }
        Ok(count.min(999))
    }
    fn snapshot(&self) -> io::Result<String> {
        let global = self.ptr(self.base + GLOBAL)?;
        let game = self.ptr(global + 0x68)?;
        if game == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let player = self.ptr(game + 0x80)?;
        if player == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let map = self.ptr(player + 0x20)?;
        let force_id = self.byte(player + 0x2a)?;
        let forces = self.ptr(map + 0x3c8)?;
        let force = self.ptr(forces + u64::from(force_id) * 8)?;
        let active = self.ptr(player + 0xbe0)?;
        let mut controller = active;
        if self.ptr(controller)? != self.base + 0x3d7e340 {
            // Player retains its character controller while remote/map controllers are active.
            controller = 0;
            for offset in [0x50, 0x58, 0x60, 0x68] {
                let candidate = self.ptr(player + offset)?;
                if candidate != 0 && self.ptr(candidate)? == self.base + 0x3d7e340 {
                    controller = candidate;
                    break;
                }
            }
        }
        if controller == 0 {
            // LuaPlayer::ticks_to_respawn uses UINT64_MAX while not respawning.
            if self.ptr(player + 0x750)? != u64::MAX {
                return Ok("0 ttl 3\n".into());
            }
            return Ok("wait ttl 3\n".into());
        }
        let character = self.ptr(controller + 0x170)?;
        if character == 0 {
            return Ok("0 ttl 3\n".into());
        }
        if self.ptr(character)? != self.base + 0x3db1388 {
            return Err(invalid("unexpected character type"));
        }
        let proto = self.ptr(character + 0x48)?;
        let max = self.f32(proto + 0x630)? * self.quality(self.byte(character + 0x91)?)?
            + self.f64(force + 0x588)?
            + self.f32(character + 0x348)?;
        let ratio = self.f32(character + 0x80)?;
        if max <= 0. || ratio > 10. {
            return Err(invalid("invalid character health"));
        }
        let (shield, max_shield, battery) = self.equipment(character)?;
        let mut line = format!("{:.3}/{max:.3}", ratio * max);
        if max_shield > 0. {
            line.push_str(&format!(" shield {shield:.3}/{max_shield:.3}"));
        }
        if let Some(b) = battery {
            line.push_str(&format!(" battery {b:.3}"));
        }
        let manager = self.ptr(force + 0x98)?;
        let tech = self.ptr(manager + 0x90)?;
        if tech != 0 {
            let prototype = self.ptr(tech)?;
            let name = self.label(prototype)?;
            let trigger = self.ptr(tech + 0x10)?;
            let progress = if trigger == 0 {
                let units = self.ptr(manager + 0x98)?;
                let progress = self.f64(manager + 0x70)?;
                if units > 0 {
                    Some(progress / units as f64)
                } else {
                    None
                }
            } else {
                let vt = self.ptr(trigger)?;
                if vt == self.base + 0x3faf268 {
                    let descriptor = self.ptr(trigger + 0x10)?;
                    let target = u32::from_le_bytes(self.read(descriptor + 0x30)?);
                    let current = u32::from_le_bytes(self.read(trigger + 0x18)?);
                    (target > 0).then(|| f64::from(current) / f64::from(target))
                } else if vt == self.base + 0x3faf178 {
                    let descriptor = self.ptr(trigger + 0x10)?;
                    let target = self.f64(descriptor + 0x18)?;
                    (target > 0.)
                        .then(|| self.f64(trigger + 0x18).map(|v| v / target))
                        .transpose()?
                } else if self.ptr(vt + 0x38)? == self.base + 0x25d3360 {
                    // One-shot triggers report zero until the technology completes.
                    Some(0.)
                } else {
                    None
                }
            };
            if let Some(progress) = progress {
                line.push_str(&format!(
                    " research {:.3} technology {name}",
                    (progress * 100.).clamp(0., 100.)
                ));
            }
            if self.ptr(manager + 0x90)? != tech {
                return Err(invalid("research changed during read"));
            }
        }
        line.push_str(&format!(" attack {} ttl 3\n", self.attacks(player)?));
        if self.ptr(global + 0x68)? != game
            || self.ptr(game + 0x80)? != player
            || self.ptr(player + 0xbe0)? != active
            || self.ptr(controller + 0x170)? != character
            || self.ptr(player + 0x20)? != map
            || self.byte(player + 0x2a)? != force_id
        {
            return Err(invalid("player changed during read"));
        }
        Ok(line)
    }
}
fn number(v: f64) -> io::Result<f64> {
    if v.is_finite() && (0.0..=1e18).contains(&v) {
        Ok(v)
    } else {
        Err(invalid("invalid numeric telemetry"))
    }
}
fn vector_count(start: u64, end: u64, stride: u64, limit: u64) -> io::Result<u64> {
    let bytes = end
        .checked_sub(start)
        .ok_or_else(|| invalid("reversed vector"))?;
    if bytes % stride != 0 || bytes / stride > limit {
        return Err(invalid("invalid vector length"));
    }
    Ok(bytes / stride)
}
fn descendant(pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..64 {
        if current == ancestor {
            return true;
        }
        let Ok(s) = fs::read_to_string(format!("/proc/{current}/status")) else {
            return false;
        };
        let Some(p) = s.lines().find_map(|l| {
            l.strip_prefix("PPid:\t")
                .and_then(|p| p.parse::<u32>().ok())
        }) else {
            return false;
        };
        if p == 0 || p == current {
            return false;
        }
        current = p;
    }
    false
}
fn find_game(ancestor: u32) -> Option<u32> {
    let mut pending = vec![ancestor];
    let mut seen = HashSet::new();
    while let Some(pid) = pending.pop() {
        if seen.len() >= 512 || !seen.insert(pid) {
            continue;
        }
        if pid != ancestor
            && fs::read_to_string(format!("/proc/{pid}/comm"))
                .ok()
                .as_deref()
                == Some("factorio\n")
            && descendant(pid, ancestor)
        {
            return Some(pid);
        }
        // Follow our launch tree instead of reading every process on the host.
        // Runtime wrappers can create children from worker threads.
        if let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) {
            for task in tasks.flatten() {
                if let Ok(children) = fs::read_to_string(task.path().join("children")) {
                    pending.extend(
                        children
                            .split_whitespace()
                            .filter_map(|s| s.parse::<u32>().ok()),
                    );
                }
            }
        }
    }
    None
}
fn publish(path: &Path, line: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| invalid("feed has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("factorio-{}.tmp", process::id()));
    fs::write(&tmp, line)?;
    fs::rename(tmp, path)
}
fn run() -> io::Result<i32> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|s| s == "--version") {
        println!(
            "g13map-factorio {} (native 2.1.21)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(0);
    }
    if args.first().is_some_and(|s| s == "--help") || args.is_empty() {
        println!("g13map-factorio GAME [ARGS...]\nSteam: g13map-factorio %command%\nRead-only diagnostic: g13map-factorio --pid PID [--once]\nSupported: native Linux x86-64 Factorio 2.1.21 build 87673. No mods or game writes.");
        return Ok(0);
    }
    if args[0] == "--pid" {
        let pid = args
            .get(1)
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid("expected PID"))?;
        if args.len() > 3 || args.get(2).is_some_and(|s| s != "--once") {
            return Err(invalid("expected --pid PID [--once]"));
        }
        let r = Reader::open(pid)?;
        loop {
            print!("{}", r.snapshot()?);
            io::stdout().flush()?;
            if args.len() == 3 {
                return Ok(0);
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    let path = env::var_os("G13MAP_HEALTH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let state = env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
                });
            state.join("g13map/health")
        });
    // Telemetry failure must never prevent the game from running or replace its exit status.
    let mut child = Command::new(&args[0]).args(&args[1..]).spawn()?;
    let mut reader = None;
    let mut next_scan = Instant::now();
    let mut warned = false;
    let mut last_line = String::new();
    let mut last_write = Instant::now() - Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if reader.is_none() && Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(1);
            if let Some(pid) = find_game(process::id()) {
                match Reader::open(pid) {
                    Ok(r) => {
                        eprintln!("g13map-factorio: observing native Factorio PID {pid}");
                        reader = Some(r);
                    }
                    Err(e) => {
                        if !warned {
                            eprintln!("g13map-factorio: {e}; game continues without telemetry");
                            warned = true;
                        }
                    }
                }
            }
        }
        let line = reader
            .as_ref()
            .and_then(|r| r.snapshot().ok())
            .unwrap_or_else(|| "wait ttl 3\n".into());
        if line != last_line || last_write.elapsed() >= Duration::from_secs(1) {
            if let Err(e) = publish(&path, &line) {
                if !warned {
                    eprintln!("g13map-factorio: feed: {e}");
                    warned = true;
                }
            }
            last_line = line;
            last_write = Instant::now();
        }
        thread::sleep(Duration::from_millis(200));
    }
}
fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("g13map-factorio: {e}");
            process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_lengths_and_values_fail_closed() {
        assert!(vector_count(10, 9, 8, 10).is_err());
        assert!(vector_count(1, 10, 8, 10).is_err());
        assert!(vector_count(0, 88 * 1001, 88, 1000).is_err());
        assert_eq!(vector_count(0, 0, 8, 1000).unwrap(), 0);
        assert!(number(f64::NAN).is_err());
        assert!(number(f64::INFINITY).is_err());
        assert!(number(-1.).is_err());
    }
    #[test]
    fn elf_gate_rejects_unrelated_executables() {
        let exe = File::open("/proc/self/exe").unwrap();
        assert_ne!(build_id(&exe).unwrap(), BUILD_ID);
        assert!(Reader::open(process::id())
            .err()
            .unwrap()
            .to_string()
            .contains("unsupported Factorio build"));
    }
}
