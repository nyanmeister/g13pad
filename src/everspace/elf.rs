// SPDX-License-Identifier: GPL-3.0-or-later
// Bounded ELF symbol lookup, following the native Factorio observer.
use super::{bytes, invalid};
use std::{fs::File, io, os::unix::fs::FileExt};

pub(super) fn native_header(f: &File) -> io::Result<[u8; 64]> {
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
pub(super) fn image_origin(f: &File) -> io::Result<u64> {
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
pub(super) fn resolve<const N: usize>(
    f: &File,
    names: [&str; N],
    fallback: [u64; N],
) -> io::Result<([u64; N], usize)> {
    let h = native_header(f)?;
    let offset = u64::from_le_bytes(h[40..48].try_into().unwrap());
    let stride = u16::from_le_bytes(h[58..60].try_into().unwrap());
    let count = u16::from_le_bytes(h[60..62].try_into().unwrap());
    let mut layout = fallback;
    let mut resolved = [false; N];
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
        let name_section = sections
            .get(link)
            .ok_or_else(|| invalid("invalid ELF string table link"))?;
        if u32::from_le_bytes(name_section[4..8].try_into().unwrap()) != 3 {
            return Err(invalid("invalid ELF string table"));
        }
        let names_at = u64::from_le_bytes(name_section[24..32].try_into().unwrap());
        let names_size = u64::from_le_bytes(name_section[32..40].try_into().unwrap());
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
            if let Some(i) = names.iter().position(|s| s.as_bytes() == &tail[..end]) {
                layout[i] = address;
                resolved[i] = true;
            }
        }
    }
    Ok((layout, resolved.iter().filter(|v| **v).count()))
}
