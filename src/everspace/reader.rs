// SPDX-License-Identifier: GPL-3.0-or-later
//! Field offsets from the native executable's DWARF; never call into the game.
use super::*;
use std::cell::Cell;
#[path = "reader/stats.rs"]
mod stats;
const SYMBOLS: [&str; 6] = [
    "GWorld",
    "GUObjectArray",
    "_ZZN9AShipBase21GetPrivateStaticClassEvE18PrivateStaticClass",
    "_ZZN16UHealthComponent21GetPrivateStaticClassEvE18PrivateStaticClass",
    "_ZZN16UShieldComponent21GetPrivateStaticClassEvE18PrivateStaticClass",
    "_ZZN20UEnergyCoreComponent21GetPrivateStaticClassEvE18PrivateStaticClass",
];
const FALLBACK: [u64; 6] = [
    0x713e530, 0x6fe7900, 0x6e298a0, 0x6e26b40, 0x6e29788, 0x6e24e68,
];
pub(super) fn globals(file: &File) -> io::Result<([u64; 6], usize)> {
    elf::resolve(file, SYMBOLS, FALLBACK)
}
pub(super) struct Reader {
    mem: File,
    globals: [u64; 6],
    image: u64,
    energy_sample: Cell<Option<stats::EnergySample>>,
}
impl Reader {
    pub(super) fn open(pid: u32) -> io::Result<Self> {
        if !native_game(pid) {
            return Err(invalid("expected native RSG-Linux-Shipping"));
        }
        // Fail a permission retry before scanning the executable's symbol tables.
        let mem = File::open(format!("/proc/{pid}/mem"))?;
        let exe = File::open(format!("/proc/{pid}/exe"))?;
        let (mut globals, count) = globals(&exe)?;
        if count != SYMBOLS.len() {
            eprintln!(
                "g13map-everspace: {count}/6 symbols; trying last known addresses with validation"
            );
        }
        let ino = exe.metadata()?.ino();
        let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
        let image = maps
            .lines()
            .find_map(|line| {
                let p: Vec<_> = line.split_whitespace().collect();
                if p.len() < 5 || p[2] != "00000000" || p[4].parse::<u64>().ok() != Some(ino) {
                    return None;
                }
                u64::from_str_radix(p[0].split('-').next()?, 16).ok()
            })
            .ok_or_else(|| invalid("executable mapping missing"))?;
        let bias = image
            .checked_sub(elf::image_origin(&exe)?)
            .ok_or_else(|| invalid("invalid load bias"))?;
        for address in &mut globals {
            *address = address
                .checked_add(bias)
                .ok_or_else(|| invalid("global overflow"))?;
        }
        if bytes::<4>(&mem, image)? != *b"\x7fELF" {
            return Err(invalid("executable changed during open"));
        }
        Ok(Self {
            mem,
            globals,
            image,
            energy_sample: Cell::new(None),
        })
    }
    pub(super) fn retired(&self) -> bool {
        bytes::<4>(&self.mem, self.image).is_err()
    }
    fn read<const N: usize>(&self, at: u64) -> io::Result<[u8; N]> {
        if !(0x10000..0x0000_8000_0000_0000).contains(&at) || at.checked_add(N as u64).is_none() {
            return Err(invalid("invalid read address"));
        }
        bytes(&self.mem, at)
    }
    fn ptr(&self, at: u64) -> io::Result<u64> {
        let value = u64::from_le_bytes(self.read(at)?);
        if value != 0 && (!(0x10000..0x0000_8000_0000_0000).contains(&value) || value % 8 != 0) {
            return Err(invalid("invalid pointer"));
        }
        Ok(value)
    }
    fn integer(&self, at: u64) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.read(at)?))
    }
    fn float(&self, at: u64) -> io::Result<f64> {
        let v = f32::from_le_bytes(self.read(at)?) as f64;
        if !v.is_finite() {
            return Err(invalid("non-finite ship stat"));
        }
        Ok(v)
    }
    fn is_a(&self, object: u64, class_global: usize) -> io::Result<bool> {
        if object == 0 {
            return Ok(false);
        }
        let wanted = self.ptr(self.globals[class_global])?;
        if wanted == 0 {
            return Ok(false);
        }
        let mut class = self.ptr(object + 16)?;
        for _ in 0..32 {
            if class == 0 {
                return Ok(false);
            }
            if class == wanted {
                return Ok(true);
            }
            class = self.ptr(class + 48)?;
        }
        Err(invalid("cyclic/oversized class ancestry"))
    }
    fn shield(&self, component: u64) -> io::Result<Option<(u32, Option<u16>)>> {
        if component == 0 {
            return Ok(None);
        }
        if !self.is_a(component, 4)? {
            return Err(invalid("unexpected shield component"));
        }
        let index = self.integer(component + 776)?;
        let serial = self.integer(component + 780)?;
        if serial == 0 || index < 0 {
            return Ok(None);
        }
        let objects = self.ptr(self.globals[1] + 16)?;
        let max = self.integer(self.globals[1] + 24)?;
        let count = self.integer(self.globals[1] + 28)?;
        if !(0..=4_000_000).contains(&count) || count > max || index >= count {
            return Err(invalid("invalid weak shield index"));
        }
        let entry = objects + index as u64 * 24;
        if self.integer(entry + 16)? != serial {
            return Ok(None);
        }
        let shield = self.ptr(entry)?;
        if shield == 0 {
            return Ok(None);
        }
        // GetMaxHitpoints = installed device capacity * (1 + bonus) * attribute.
        // Reading a dirty cached attribute would invent an effective maximum.
        if self.read::<1>(component + 672)?[0] != 0 {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let capacity = self.float(shield + 1092)?;
        let bonus = self.float(component + 688)?;
        let multiplier = self.float(component + 652)?;
        let maximum = capacity * (1.0 + bonus) * multiplier;
        if maximum == 0.0 {
            return Ok(None);
        }
        let current = self.float(component + 464)?;
        let delay = self.shield_delay(component).ok().flatten();
        Ok(Some((ratio(current, maximum)?, delay)))
    }
    pub(super) fn snapshot(&self) -> io::Result<String> {
        let world = self.ptr(self.globals[0])?;
        if world == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let game = self.ptr(world + 320)?;
        if game == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let players = self.ptr(game + 56)?;
        let count = self.integer(game + 64)?;
        let capacity = self.integer(game + 68)?;
        if !(0..=4).contains(&count) || count > capacity || capacity > 16 {
            return Err(invalid("invalid local player array"));
        }
        if count == 0 || players == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let player = self.ptr(players)?;
        if player == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let controller = self.ptr(player + 48)?;
        if controller == 0 {
            return Ok("wait ttl 3\n".into());
        }
        let pawn = self.ptr(controller + 912)?;
        if !self.is_a(pawn, 2)? {
            return Ok("wait ttl 3\n".into());
        }
        let health = self.ptr(pawn + 1000)?;
        if !self.is_a(health, 3)? {
            return Err(invalid("unexpected hull component"));
        }
        let current_hull = self.float(health + 516)?;
        let max_hull = self.float(health + 512)?;
        let hull = ratio(current_hull, max_hull)?;
        let repair = self
            .repair_budget(game, health, current_hull, max_hull)
            .ok()
            .flatten();
        let shield = self.shield(self.ptr(pawn + 1008)?)?;
        let energy = self.ptr(pawn + 1048)?;
        let (energy, energy_time) = if energy == 0 {
            (None, None)
        } else {
            if !self.is_a(energy, 5)? {
                return Err(invalid("unexpected energy component"));
            }
            if self.read::<1>(energy + 496)?[0] != 0 {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            let maximum = self.float(energy + 476)? - self.float(energy + 608)?;
            if maximum == 0.0 {
                (None, None)
            } else {
                let current = self.float(energy + 604)?;
                (
                    Some(ratio(current, maximum)?),
                    self.energy_time(world, pawn, energy, current, maximum)
                        .ok()
                        .flatten(),
                )
            }
        };
        if self.ptr(self.globals[0])? != world || self.ptr(controller + 912)? != pawn {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let field = |v: Option<u32>| v.map_or_else(|| "none".into(), |n| n.to_string());
        let mut line = format!(
            "ship 1 hull {hull} shield {} energy {} ttl 3\n",
            field(shield.map(|s| s.0)),
            field(energy)
        );
        line.pop();
        if let Some(value) = repair {
            line.push_str(&format!(" repair {value}"));
        }
        if let Some(value) = shield.and_then(|s| s.1) {
            line.push_str(&format!(" shield_delay {value}"));
        }
        if let Some(value) = energy_time {
            line.push_str(&format!(" energy_time {value}"));
        }
        line.push('\n');
        Ok(line)
    }
}
fn ratio(current: f64, maximum: f64) -> io::Result<u32> {
    if !current.is_finite()
        || !maximum.is_finite()
        || !(0.0..=1_000_000_000.0).contains(&maximum)
        || maximum == 0.0
        || current < -maximum
        || current > maximum * 4.0
    {
        return Err(invalid("invalid resource range"));
    }
    Ok((current.max(0.0) / maximum * 100.0).round().min(999.0) as u32)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
