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
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout([u64; 17]);
const SYMBOLS: [&str; 17] = [
    "global",
    "_ZN13PrototypeListI16QualityPrototypeE16indexToPrototypeE",
    "_ZN13PrototypeListI18EquipmentPrototypeE16indexToPrototypeE",
    "_ZTV5Armor",
    "_ZTV16BatteryEquipment",
    "_ZTV21EnergyShieldEquipment",
    "_ZTV19CharacterController",
    "_ZTV9Character",
    "_ZTV26CraftItemTechnologyTrigger",
    "_ZTV27CraftFluidTechnologyTrigger",
    "_ZNK17TechnologyTrigger11getProgressEv",
    "_ZTV3Car",
    "_ZTV10Locomotive",
    "_ZTV10CargoWagon",
    "_ZTV10FluidWagon",
    "_ZTV14ArtilleryWagon",
    "_ZTV13SpiderVehicle",
];
const FALLBACK: Layout = Layout([
    GLOBAL, 0x425faf0, 0x425f310, 0x3f97738, 0x3e7fab8, 0x3e80718, 0x3d7e330, 0x3db1378, 0x3faf258,
    0x3faf168, 0x25d3360, 0x3da8458, 0x3e0e0b0, 0x3daecf0, 0x3ddf298, 0x3d97540, 0x3e5bd10,
]);

fn native_header(f: &File) -> io::Result<[u8; 64]> {
    let h = bytes::<64>(f, 0)?;
    if &h[..6] != b"\x7fELF\x02\x01" || u16::from_le_bytes(h[18..20].try_into().unwrap()) != 62 {
        return Err(invalid("requires native Linux x86-64 ELF"));
    }
    Ok(h)
}
fn file_range(f: &File, at: u64, length: u64) -> io::Result<()> {
    if at
        .checked_add(length)
        .is_none_or(|end| end > f.metadata().map(|m| m.len()).unwrap_or(0))
    {
        return Err(invalid("ELF section outside executable"));
    }
    Ok(())
}
fn image_origin(f: &File) -> io::Result<u64> {
    let h = native_header(f)?;
    let offset = u64::from_le_bytes(h[32..40].try_into().unwrap());
    let stride = u16::from_le_bytes(h[54..56].try_into().unwrap());
    let count = u16::from_le_bytes(h[56..58].try_into().unwrap());
    if stride != 56 || count > 128 {
        return Err(invalid("invalid ELF load headers"));
    }
    file_range(f, offset, u64::from(count) * 56)?;
    for i in 0..count {
        let p = bytes::<56>(f, offset + u64::from(i) * 56)?;
        if u32::from_le_bytes(p[..4].try_into().unwrap()) == 1
            && u64::from_le_bytes(p[8..16].try_into().unwrap()) == 0
        {
            return Ok(u64::from_le_bytes(p[16..24].try_into().unwrap()));
        }
    }
    Err(invalid("executable origin missing"))
}

// Read the small symbol/string sections once, never scan the game heap. Member
// offsets remain provisional on new builds; every snapshot still validates them.
fn resolve_layout(f: &File) -> io::Result<(Layout, usize)> {
    let h = native_header(f)?;
    let offset = u64::from_le_bytes(h[40..48].try_into().unwrap());
    let stride = u16::from_le_bytes(h[58..60].try_into().unwrap());
    let count = u16::from_le_bytes(h[60..62].try_into().unwrap());
    let mut layout = FALLBACK;
    let mut resolved = [false; SYMBOLS.len()];
    if count == 0 {
        return Ok((layout, 0)); // stripped build: try the previous layout
    }
    if stride != 64 || count > 4096 {
        return Err(invalid("invalid ELF section headers"));
    }
    file_range(f, offset, u64::from(count) * 64)?;
    let mut sections = Vec::with_capacity(count as usize);
    for i in 0..count {
        sections.push(bytes::<64>(f, offset + u64::from(i) * 64)?);
    }
    for section in &sections {
        if u32::from_le_bytes(section[4..8].try_into().unwrap()) != 2 {
            continue;
        }
        let table_at = u64::from_le_bytes(section[24..32].try_into().unwrap());
        let size = u64::from_le_bytes(section[32..40].try_into().unwrap());
        let link = u32::from_le_bytes(section[40..44].try_into().unwrap()) as usize;
        let entry = u64::from_le_bytes(section[56..64].try_into().unwrap());
        if entry != 24 || size % 24 != 0 || size > 64 * 1024 * 1024 {
            return Err(invalid("invalid ELF symbol table"));
        }
        let names = sections
            .get(link)
            .ok_or_else(|| invalid("invalid ELF string table link"))?;
        if u32::from_le_bytes(names[4..8].try_into().unwrap()) != 3 {
            return Err(invalid("invalid ELF string table"));
        }
        let names_at = u64::from_le_bytes(names[24..32].try_into().unwrap());
        let names_size = u64::from_le_bytes(names[32..40].try_into().unwrap());
        if names_size > 64 * 1024 * 1024 {
            return Err(invalid("oversize ELF string table"));
        }
        file_range(f, names_at, names_size)?;
        file_range(f, table_at, size)?;
        let mut strings = vec![0; names_size as usize];
        f.read_exact_at(&mut strings, names_at)?;
        let mut symbols = vec![0; size as usize];
        f.read_exact_at(&mut symbols, table_at)?;
        for symbol in symbols.as_chunks::<24>().0 {
            let name = u32::from_le_bytes(symbol[..4].try_into().unwrap()) as usize;
            let address = u64::from_le_bytes(symbol[8..16].try_into().unwrap());
            if symbol[6..8] == [0, 0] || address == 0 || address > 1 << 30 {
                continue;
            }
            let tail = strings
                .get(name..)
                .ok_or_else(|| invalid("invalid ELF symbol name"))?;
            let end = tail
                .iter()
                .position(|v| *v == 0)
                .ok_or_else(|| invalid("unterminated ELF symbol name"))?;
            if let Some(i) = SYMBOLS.iter().position(|s| s.as_bytes() == &tail[..end]) {
                layout.0[i] = address;
                resolved[i] = true;
            }
        }
    }
    Ok((layout, resolved.iter().filter(|v| **v).count()))
}
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
    image: u64,
    base: u64,
    layout: Layout,
}
fn native_game(pid: u32) -> bool {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| {
            p.file_name()
                .map(|s| s == "factorio" || s == "factorio (deleted)")
        })
        .unwrap_or(false)
}
impl Reader {
    fn open(pid: u32) -> io::Result<Self> {
        let exe = File::open(format!("/proc/{pid}/exe"))?;
        native_header(&exe)?;
        if fs::read_to_string(format!("/proc/{pid}/comm"))? != "factorio\n" || !native_game(pid) {
            return Err(invalid("expected a native Factorio process"));
        }
        let id = build_id(&exe).unwrap_or_else(|_| "unidentified".into());
        let (layout, found) = resolve_layout(&exe)?;
        if id != BUILD_ID {
            eprintln!("g13map-factorio: trying unverified build {id}; resolved {found}/{} symbols, validating readings", SYMBOLS.len());
        }
        let ino = exe.metadata()?.ino();
        let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
        let mapping = maps
            .lines()
            .find_map(|l| {
                let p: Vec<_> = l.split_whitespace().collect();
                if p.len() < 5 || p[2] != "00000000" || p[4].parse::<u64>().ok() != Some(ino) {
                    return None;
                }
                u64::from_str_radix(p[0].split('-').next()?, 16).ok()
            })
            .ok_or_else(|| invalid("executable load mapping missing"))?;
        let base = mapping
            .checked_sub(image_origin(&exe)?)
            .ok_or_else(|| invalid("invalid executable origin"))?;
        let mem = File::open(format!("/proc/{pid}/mem"))?;
        if bytes::<4>(&mem, mapping)? != *b"\x7fELF" {
            return Err(invalid("executable mapping changed while opening reader"));
        }
        Ok(Self {
            mem,
            image: mapping,
            base,
            layout,
        })
    }
    fn retired(&self) -> bool {
        // /proc/PID/mem pins an address space, not a PID. After exit or exec it
        // returns EOF, even if Factorio restarted with the same PID and binary.
        // Other read errors must still go through the invalid-reading guard.
        matches!(self.mem.read_at(&mut [0u8; 1], self.image), Ok(0))
    }
    fn read<const N: usize>(&self, at: u64) -> io::Result<[u8; N]> {
        if !(0x10000..0x0000_8000_0000_0000).contains(&at) || at.checked_add(N as u64).is_none() {
            return Err(invalid("invalid memory address"));
        }
        bytes(&self.mem, at)
    }
    fn ptr(&self, at: u64) -> io::Result<u64> {
        let value = self.word(at)?;
        if value != 0 && !(0x10000..0x0000_8000_0000_0000).contains(&value) {
            return Err(invalid("invalid pointer value"));
        }
        Ok(value)
    }
    fn word(&self, at: u64) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.read(at)?))
    }
    fn address(&self, symbol: usize) -> u64 {
        self.base + self.layout.0[symbol]
    }
    fn vtable(&self, symbol: usize) -> u64 {
        self.address(symbol) + 16
    }
    fn byte(&self, at: u64) -> io::Result<u8> {
        Ok(self.read::<1>(at)?[0])
    }
    fn f32(&self, at: u64) -> io::Result<f64> {
        let v = f32::from_le_bytes(self.read(at)?) as f64;
        finite(v)
    }
    fn f64(&self, at: u64) -> io::Result<f64> {
        finite(f64::from_le_bytes(self.read(at)?))
    }
    fn quality(&self, id: u8) -> io::Result<f64> {
        let table = self.ptr(self.address(1))?;
        let proto = self.ptr(table + u64::from(id) * 8)?;
        let quality = self.f32(proto + 0x370)?;
        if !(0.001..=1e6).contains(&quality) {
            return Err(invalid("invalid quality multiplier"));
        }
        Ok(quality)
    }
    fn label(&self, prototype: u64) -> io::Result<String> {
        let addr = self.ptr(prototype + 8)?;
        let length = self.word(prototype + 16)?;
        if length == 0 || length > 256 {
            return Err(invalid("invalid prototype name length"));
        }
        let mut b = vec![0; length as usize];
        self.mem.read_exact_at(&mut b, addr)?;
        let s = std::str::from_utf8(&b).map_err(|_| invalid("invalid prototype name"))?;
        if s.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(invalid("invalid prototype name characters"));
        }
        Ok(s.chars()
            .take(256)
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
        if item == 0 {
            return Ok((0., 0., None));
        }
        if self.ptr(item)? != self.vtable(3) {
            return Err(invalid("unexpected armor type"));
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
            if vt == self.vtable(4) {
                let source = self.ptr(e + 0x30)?;
                charge += self.f64(source + 0x20)?;
                capacity += self.f64(source + 0x28)?;
            } else if vt == self.vtable(5) {
                shield += self.f32(e + 0x48)?;
                let id = u16::from_le_bytes(self.read(e + 0x20)?);
                let table = self.ptr(self.address(2))?;
                let proto = self.ptr(table + u64::from(id) * 8)?;
                max_shield += self.f32(proto + 0x330)? * self.quality(self.byte(e + 0x22)?)?;
            }
        }
        if bounds != self.read::<16>(grid + 0x20)? {
            return Err(invalid("equipment changed during read"));
        }
        bounded_pool(shield, max_shield, 1e9)?;
        bounded_pool(charge, capacity, 1e15)?;
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
        if count > 65536 {
            return Err(invalid("invalid attack count"));
        }
        Ok(count.min(999))
    }
    fn snapshot(&self) -> io::Result<String> {
        let global = self.ptr(self.address(0))?;
        if global == 0 {
            return Ok("wait ttl 3\n".into());
        }
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
        if self.ptr(controller)? != self.vtable(6) {
            // Player retains its character controller while remote/map controllers are active.
            controller = 0;
            for offset in [0x50, 0x58, 0x60, 0x68] {
                let candidate = self.ptr(player + offset)?;
                if candidate != 0 && self.ptr(candidate)? == self.vtable(6) {
                    controller = candidate;
                    break;
                }
            }
        }
        if controller == 0 {
            // LuaPlayer::ticks_to_respawn uses UINT64_MAX while not respawning.
            let respawn = self.word(player + 0x750)?;
            if respawn != u64::MAX {
                if respawn > 216000 {
                    return Err(invalid("invalid respawn timer"));
                }
                return Ok("0 ttl 3\n".into());
            }
            return Ok("wait ttl 3\n".into());
        }
        let character = self.ptr(controller + 0x170)?;
        if character == 0 {
            return Ok("0 ttl 3\n".into());
        }
        if self.ptr(character)? != self.vtable(7) {
            return Err(invalid("unexpected character type"));
        }
        let proto = self.ptr(character + 0x48)?;
        let max = self.f32(proto + 0x630)? * self.quality(self.byte(character + 0x91)?)?
            + self.f64(force + 0x588)?
            + self.f32(character + 0x348)?;
        let ratio = self.f32(character + 0x80)?;
        health(max, ratio)?;
        let (shield, max_shield, battery) = self.equipment(character)?;
        let vehicle = self.ptr(character + 0x550)?;
        let mut line = if vehicle != 0 {
            let vt = self.ptr(vehicle)?;
            if !(11..SYMBOLS.len()).any(|i| vt == self.vtable(i)) {
                return Err(invalid("unexpected vehicle type"));
            }
            if self.byte(vehicle + 0x6e)? & 0x10 != 0 {
                return Err(invalid("vehicle is being removed"));
            }
            let prototype = self.ptr(vehicle + 0x48)?;
            let vehicle_max =
                self.f32(prototype + 0x630)? * self.quality(self.byte(vehicle + 0x91)?)?;
            let vehicle_ratio = self.f32(vehicle + 0x80)?;
            health(vehicle_max, vehicle_ratio)?;
            format!(
                "{:.3}/{vehicle_max:.3} vehicle {} pilot {:.3}/{max:.3}",
                vehicle_ratio * vehicle_max,
                self.label(prototype)?,
                ratio * max
            )
        } else {
            format!("{:.3}/{max:.3}", ratio * max)
        };
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
                let units = self.word(manager + 0x98)?;
                let progress = self.f64(manager + 0x70)?;
                if units > 0 {
                    Some(progress / units as f64)
                } else {
                    None
                }
            } else {
                let vt = self.ptr(trigger)?;
                if vt == self.vtable(8) {
                    let descriptor = self.ptr(trigger + 0x10)?;
                    let target = u32::from_le_bytes(self.read(descriptor + 0x30)?);
                    let current = u32::from_le_bytes(self.read(trigger + 0x18)?);
                    (target > 0).then(|| f64::from(current) / f64::from(target))
                } else if vt == self.vtable(9) {
                    let descriptor = self.ptr(trigger + 0x10)?;
                    let target = self.f64(descriptor + 0x18)?;
                    (target > 0.)
                        .then(|| self.f64(trigger + 0x18).map(|v| v / target))
                        .transpose()?
                } else if self.ptr(vt + 0x38)? == self.address(10) {
                    // One-shot triggers report zero until the technology completes.
                    Some(0.)
                } else {
                    None
                }
            };
            if let Some(progress) = progress {
                fraction(progress)?;
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
            || self.ptr(character + 0x550)? != vehicle
            || self.ptr(player + 0x20)? != map
            || self.byte(player + 0x2a)? != force_id
        {
            return Err(invalid("player changed during read"));
        }
        Ok(line)
    }
}
fn fraction(value: f64) -> io::Result<()> {
    if value.is_finite() && (0.0..=1.001).contains(&value) {
        Ok(())
    } else {
        Err(invalid("invalid resource fraction"))
    }
}
fn health(maximum: f64, ratio: f64) -> io::Result<()> {
    if !maximum.is_finite() || !(0.001..=1e9).contains(&maximum) {
        return Err(invalid("invalid character maximum health"));
    }
    number(ratio)?;
    if ratio * maximum > 1e9 {
        return Err(invalid("implausible character health"));
    }
    Ok(())
}
fn bounded_pool(current: f64, maximum: f64, limit: f64) -> io::Result<()> {
    number(current)?;
    number(maximum)?;
    if maximum > limit || current > limit || (maximum == 0. && current != 0.) {
        return Err(invalid("invalid equipment pool"));
    }
    Ok(())
}
#[derive(Default)]
struct Guard {
    failures: u8,
    disabled: bool,
}
impl Guard {
    fn observe(&mut self, snapshot: io::Result<String>) -> io::Result<String> {
        match snapshot {
            Ok(line) => {
                self.failures = 0; // menus/loading are valid states, not failures
                Ok(line)
            }
            Err(e) => {
                self.failures = self.failures.saturating_add(1);
                if self.failures >= 10 {
                    self.disabled = true;
                    return Err(e);
                }
                Ok("wait ttl 3\n".into())
            }
        }
    }
}
fn number(v: f64) -> io::Result<f64> {
    if v.is_finite() && (0.0..=1e18).contains(&v) {
        Ok(v)
    } else {
        Err(invalid("invalid numeric telemetry"))
    }
}
fn finite(value: f64) -> io::Result<f64> {
    if value.is_finite() && value.abs() <= 1e18 {
        Ok(value)
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
            && native_game(pid) // skip shell scripts named factorio before their exec
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
            "g13map-factorio {} (native; tested 2.1.21)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(0);
    }
    if args.first().is_some_and(|s| s == "--help") || args.is_empty() {
        println!("g13map-factorio GAME [ARGS...]\nSteam: g13map-factorio %command%\nRead-only diagnostic: g13map-factorio --pid PID [--once]\nNative Linux x86-64; tested 2.1.21 build 87673. Other builds are tried with validated readings. No mods or game writes.");
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
    let mut guard = Guard::default();
    let mut next_scan = Instant::now();
    let mut warned = false;
    let mut last_line = String::new();
    let mut last_write = Instant::now() - Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if reader.as_ref().is_some_and(Reader::retired) {
            eprintln!("g13map-factorio: game address space ended; waiting for restarted Factorio");
            reader = None;
            guard = Guard::default();
            warned = false;
            next_scan = Instant::now();
        }
        if reader.is_none() && !guard.disabled && Instant::now() >= next_scan {
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
        let line = if guard.disabled {
            "wait ttl 3\n".into()
        } else if let Some(r) = &reader {
            match guard.observe(r.snapshot()) {
                Ok(line) => line,
                Err(e) => {
                    eprintln!("g13map-factorio: telemetry stopped after repeated invalid readings: {e}; game continues");
                    // Retain the handle to detect a later exec/exit. Do not retry
                    // junk data within the same address space.
                    "wait ttl 3\n".into()
                }
            }
        } else {
            "wait ttl 3\n".into()
        };
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
    fn restart_process_fixture() {
        use std::os::unix::process::CommandExt;
        let Some(dir) = env::var_os("G13_FACTORIO_EXEC_FIXTURE").map(PathBuf::from) else {
            return;
        };
        let phase = env::var("G13_FACTORIO_EXEC_PHASE").unwrap();
        fs::write(dir.join("phase"), &phase).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("release-{phase}")).exists() {
            assert!(Instant::now() < deadline, "restart fixture timed out");
            thread::sleep(Duration::from_millis(10));
        }
        if phase == "1" {
            panic!(
                "exec failed: {}",
                Command::new(dir.join("factorio"))
                    .args(["--exact", "tests::restart_process_fixture", "--nocapture"])
                    .env("G13_FACTORIO_EXEC_PHASE", "2")
                    .exec()
            );
        }
    }
    #[test]
    fn same_pid_exec_and_exit_retire_memory_handle() {
        let dir = env::temp_dir().join(format!("g13-factorio-exec-{}", process::id()));
        fs::create_dir(&dir).unwrap();
        fs::copy(env::current_exe().unwrap(), dir.join("factorio")).unwrap();
        let mut child = Command::new(dir.join("factorio"))
            .args(["--exact", "tests::restart_process_fixture", "--nocapture"])
            .env("G13_FACTORIO_EXEC_FIXTURE", &dir)
            .env("G13_FACTORIO_EXEC_PHASE", "1")
            .stdout(process::Stdio::null())
            .spawn()
            .unwrap();
        let phase = |expected: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while fs::read_to_string(dir.join("phase")).ok().as_deref() != Some(expected) {
                assert!(Instant::now() < deadline, "child phase timed out");
                thread::sleep(Duration::from_millis(10));
            }
        };
        phase("1");
        let old = Reader::open(child.id()).unwrap();
        assert!(!old.retired());
        fs::write(dir.join("release-1"), "").unwrap();
        phase("2");
        assert!(old.retired()); // same PID and same executable, a different mm
        let restarted = Reader::open(child.id()).unwrap();
        assert!(!restarted.retired());
        fs::write(dir.join("release-2"), "").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(restarted.retired());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn launch_script_named_factorio_is_not_the_native_game() {
        use std::os::unix::fs::PermissionsExt;
        let dir = env::temp_dir().join(format!("g13-factorio-launcher-{}", process::id()));
        fs::create_dir(&dir).unwrap();
        let script = dir.join("factorio");
        fs::write(&script, "#!/bin/sh\nsleep 1\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut child = Command::new(&script).spawn().unwrap();
        let name = fs::read_to_string(format!("/proc/{}/comm", child.id())).unwrap();
        let native = native_game(child.id());
        child.wait().unwrap();
        fs::remove_file(script).unwrap();
        fs::remove_dir(dir).unwrap();
        assert_eq!(name, "factorio\n");
        assert!(!native);
    }
    #[test]
    fn unknown_elf_resolves_moved_symbols_and_checks_section_bounds() {
        let mut elf = vec![0; 400];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[40..48].copy_from_slice(&64u64.to_le_bytes());
        elf[58..60].copy_from_slice(&64u16.to_le_bytes());
        elf[60..62].copy_from_slice(&3u16.to_le_bytes());
        elf[132..136].copy_from_slice(&2u32.to_le_bytes());
        elf[152..160].copy_from_slice(&256u64.to_le_bytes());
        elf[160..168].copy_from_slice(&48u64.to_le_bytes());
        elf[168..172].copy_from_slice(&2u32.to_le_bytes());
        elf[184..192].copy_from_slice(&24u64.to_le_bytes());
        elf[196..200].copy_from_slice(&3u32.to_le_bytes());
        elf[216..224].copy_from_slice(&304u64.to_le_bytes());
        let names = b"\0global\0_ZTV9Character\0";
        elf[224..232].copy_from_slice(&(names.len() as u64).to_le_bytes());
        elf[304..304 + names.len()].copy_from_slice(names);
        for (at, name, address) in [(256, 1u32, GLOBAL + 4096), (280, 8, FALLBACK.0[7] + 8192)] {
            elf[at..at + 4].copy_from_slice(&name.to_le_bytes());
            elf[at + 6..at + 8].copy_from_slice(&1u16.to_le_bytes());
            elf[at + 8..at + 16].copy_from_slice(&address.to_le_bytes());
        }
        let path = env::temp_dir().join(format!("g13-factorio-symbols-{}", process::id()));
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        fs::remove_file(path).unwrap();
        f.write_all_at(&elf, 0).unwrap();
        assert!(build_id(&f).is_err()); // no known ID required to use symbol data
        let (layout, found) = resolve_layout(&f).unwrap();
        assert_eq!(found, 2);
        assert_eq!(layout.0[0], GLOBAL + 4096);
        assert_eq!(layout.0[7], FALLBACK.0[7] + 8192);
        assert_eq!(layout.0[3], FALLBACK.0[3]);
        f.write_all_at(&u64::MAX.to_le_bytes(), 40).unwrap();
        assert!(resolve_layout(&f).is_err()); // reject overflow/truncation before allocating
    }
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
    fn elf_reader_rejects_unrelated_processes() {
        let exe = File::open("/proc/self/exe").unwrap();
        assert_ne!(build_id(&exe).unwrap(), BUILD_ID);
        assert!(Reader::open(process::id())
            .err()
            .unwrap()
            .to_string()
            .contains("expected a native Factorio process"));
    }
    #[test]
    fn junk_values_stop_telemetry_but_wait_and_transient_reads_do_not() {
        assert!(health(250., 0.6).is_ok());
        assert!(health(0., 0.6).is_err());
        assert!(health(1e10, 0.6).is_err());
        assert!(health(250., 3.).is_ok()); // modded overheal is not junk
        assert!(health(1e8, 2.).is_ok());
        assert!(health(250., 1e10).is_err());
        assert!(fraction(f64::NAN).is_err());
        assert!(bounded_pool(30., 150., 1e9).is_ok());
        assert!(bounded_pool(300., 150., 1e9).is_ok()); // overcharged equipment
        assert!(bounded_pool(1e10, 150., 1e9).is_err());
        let mut guard = Guard::default();
        for _ in 0..9 {
            assert!(guard.observe(Err(invalid("changing player"))).is_ok());
        }
        for _ in 0..100 {
            guard.observe(Ok("wait ttl 3\n".into())).unwrap();
        }
        for _ in 0..9 {
            assert!(guard.observe(Err(invalid("junk"))).is_ok());
        }
        assert!(guard.observe(Err(invalid("junk"))).is_err());
        assert!(guard.disabled);
    }
}
