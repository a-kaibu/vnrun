//! FPK archives: a flat list of named files, some of them packed with ZLC2.
//!
//! ```text
//! count:i32  (top bit set: the index is at the end and XORed)
//! plain:     count x { offset:u32 size:u32 name:[u8; 16 or 24] }    right after the count
//! encrypted: count x { offset:u32 size:u32 name:[u8; 24] ?:u32 }    at index_offset, each byte
//!            XORed with key[i % 4]; the file ends with key:[u8; 4] index_offset:u32
//! "FPK 2.00": at 0x1C count:i32, at 0x20 count x { offset:u32 size:u32 name:[u8; 24] }
//! ZLC2:      "ZLC2" size:u32 then LZSS: a flag byte (high bit first) for 8 items, a set bit a
//!            back reference (lo, hi: offset lo | (hi & 0xF0) << 4, 0 = 4096, length (hi & 15) + 3)
//! ```

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
}

#[derive(Debug, Default)]
pub struct Index {
    pub entries: HashMap<String, Entry>,
    /// names in archive order
    pub names: Vec<String>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn name_of(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    nexas::sjis::decode(&b[..end])
}

impl Index {
    /// Reads the index of the archive `path`.
    pub fn open(files: &mut dyn Files, path: &str) -> Result<Index, String> {
        let len = files.size(path).ok_or_else(|| format!("{path}: missing"))?;
        let head = files
            .read_range(path, 0, 0x20.min(len as usize))
            .ok_or_else(|| format!("{path}: unreadable"))?;
        if head.len() < 8 {
            return Err(format!("{path}: too short"));
        }
        let mut index = Index::default();
        if head.starts_with(b"FPK 2.00") && head.len() >= 0x20 {
            let count = u32_at(&head, 0x1C) as usize;
            let b = files
                .read_range(path, 0x20, count * 0x20)
                .ok_or("FPK: index cut short")?;
            for e in b.chunks_exact(0x20) {
                let name = name_of(&e[8..]);
                if !name.starts_with('/') {
                    index.add(name, u32_at(e, 0) as u64, u32_at(e, 4), len)?;
                }
            }
            return Ok(index);
        }
        let raw = u32_at(&head, 0);
        let count = (raw & 0x7FFF_FFFF) as usize;
        if count == 0 || count > 1_000_000 {
            return Err(format!("{path}: not an FPK archive"));
        }
        if raw & 0x8000_0000 != 0 {
            let tail = files
                .read_range(path, len - 8, 8)
                .ok_or("FPK: no index key")?;
            let key = [tail[0], tail[1], tail[2], tail[3]];
            let at = u32_at(&tail, 4) as u64;
            let mut b = files
                .read_range(path, at, count * 36)
                .ok_or("FPK: index cut short")?;
            for (i, v) in b.iter_mut().enumerate() {
                *v ^= key[i & 3];
            }
            for e in b.chunks_exact(36) {
                index.add(name_of(&e[8..32]), u32_at(e, 0) as u64, u32_at(e, 4), len)?;
            }
            return Ok(index);
        }
        for name_len in [0x10usize, 0x18] {
            let size = count * (8 + name_len);
            let Some(b) = files.read_range(path, 4, size) else {
                continue;
            };
            let mut try_index = Index::default();
            let ok = b.chunks_exact(8 + name_len).all(|e| {
                let name = name_of(&e[8..]);
                let offset = u32_at(e, 0) as u64;
                !name.trim().is_empty()
                    && offset >= 4 + size as u64
                    && try_index.add(name, offset, u32_at(e, 4), len).is_ok()
            });
            if ok {
                return Ok(try_index);
            }
        }
        Err(format!("{path}: not an FPK archive"))
    }

    fn add(&mut self, name: String, offset: u64, size: u32, len: u64) -> Result<(), String> {
        if offset + size as u64 > len {
            return Err(format!("FPK: {name} is past the end"));
        }
        self.entries
            .insert(name.to_lowercase(), Entry { offset, size });
        self.names.push(name);
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<Entry> {
        self.entries.get(&name.to_lowercase()).copied()
    }
}

/// The contents of an entry: unpacked while it starts with "ZLC2".
pub fn unpack(mut b: Vec<u8>) -> Vec<u8> {
    while b.len() > 8 && b.starts_with(b"ZLC2") {
        b = zlc2(&b);
    }
    b
}

fn zlc2(b: &[u8]) -> Vec<u8> {
    let n = (u32_at(b, 4) as usize).min(256 << 20);
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut p = 8;
    while p < b.len() && out.len() < n {
        let ctl = b[p];
        p += 1;
        let mut mask = 0x80u8;
        while mask != 0 && p < b.len() && out.len() < n {
            if ctl & mask != 0 {
                if p + 2 > b.len() {
                    return out;
                }
                let mut off = b[p] as usize | ((b[p + 1] as usize & 0xF0) << 4);
                let count = (b[p + 1] as usize & 0x0F) + 3;
                p += 2;
                if off == 0 {
                    off = 4096;
                }
                for _ in 0..count {
                    if out.len() >= n {
                        break;
                    }
                    let v = if off <= out.len() {
                        out[out.len() - off]
                    } else {
                        0
                    };
                    out.push(v);
                }
            } else {
                out.push(b[p]);
                p += 1;
            }
            mask >>= 1;
        }
    }
    out
}

/// All the archives of the game folder, and which one holds each name (the first one).
#[derive(Default)]
pub struct Archives {
    pub packs: Vec<(String, Index)>,
    by_name: HashMap<String, (usize, Entry)>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives::default();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|f| f.to_lowercase().ends_with(".fpk"))
            .collect();
        // data first, then the rest by name (so data.fpk wins for the scripts)
        names.sort_by_key(|n| (!n.eq_ignore_ascii_case("data.fpk"), n.to_lowercase()));
        for name in names {
            // movies are big and hold nothing we show
            if name.to_lowercase().starts_with("mov") {
                continue;
            }
            if let Ok(index) = Index::open(files, &name) {
                let k = a.packs.len();
                for (n, e) in &index.entries {
                    a.by_name.entry(n.clone()).or_insert((k, *e));
                }
                a.packs.push((name, index));
            }
        }
        a
    }

    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let (k, e) = *self.by_name.get(&name.to_lowercase())?;
        let b = files.read_range(&self.packs[k].0, e.offset, e.size as usize)?;
        Some(unpack(b))
    }

    /// Reads `name` from the archive named `pack` (case ignored) only.
    pub fn read_in(&self, files: &mut dyn Files, pack: &str, name: &str) -> Option<Vec<u8>> {
        let (path, index) = self
            .packs
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(pack))?;
        let e = index.find(name)?;
        let b = files.read_range(path, e.offset, e.size as usize)?;
        Some(unpack(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlc2_literals_and_back_references() {
        // "abc" then a 6 byte copy from 3 back
        let mut b = b"ZLC2".to_vec();
        b.extend_from_slice(&9u32.to_le_bytes());
        b.push(0b0001_0000);
        b.extend_from_slice(b"abc");
        b.extend_from_slice(&[3, 3]);
        assert_eq!(unpack(b), b"abcabcabc");
    }
}
