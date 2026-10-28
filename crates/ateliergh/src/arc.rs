//! `.arc`: u32 count, u32 (1), then `count` entries of 44 bytes: a 32-byte Shift-JIS name,
//! the size, the stored size and the offset. The header and entries are masked: each byte has
//! `key[pos % key.len()]` added (pos from the start of the file). The key differs between
//! archives (3 bytes for msg/dat, 5 for sys), so it is found from the entries themselves: the
//! zero padding after the names gives the key at each position. A file whose stored size
//! differs from its size is LZSS-packed (4 KiB window from 0xfee, flag bit 1 = literal).

use std::collections::HashMap;

use nexas::files::Files;

const ENTRY: usize = 44;

#[derive(Clone, Debug)]
pub struct Entry {
    pub arc: String,
    pub size: u32,
    pub stored: u32,
    pub offset: u32,
}

/// The files of all archives, by lower-case name (a later archive replaces an earlier one).
#[derive(Default)]
pub struct Index {
    pub files: HashMap<String, Entry>,
}

/// The key that makes the most bytes of the entries the same at each position: the padding
/// is zero, so that byte is the key.
fn guess_key(idx: &[u8], base: usize) -> Vec<u8> {
    let mut best: Option<(usize, Vec<u8>)> = None;
    for p in 1..=16 {
        let mut key = Vec::with_capacity(p);
        let mut score = 0;
        for r in 0..p {
            let mut count = [0usize; 256];
            for (i, &b) in idx.iter().enumerate() {
                if (base + i) % p == r {
                    count[b as usize] += 1;
                }
            }
            let (k, c) = count
                .iter()
                .enumerate()
                .max_by_key(|(_, c)| **c)
                .map(|(k, c)| (k as u8, *c))
                .unwrap();
            key.push(k);
            score += c;
        }
        if best.as_ref().is_none_or(|b| score * 100 > b.0 * 105) {
            best = Some((score, key));
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

pub fn sjis(b: &[u8]) -> String {
    nexas::sjis::decode(b)
}

impl Index {
    pub fn add(&mut self, files: &mut dyn Files, arc: &str) -> Result<(), String> {
        let head = files
            .read_range(arc, 0, 8)
            .ok_or(format!("{arc}: unreadable"))?;
        let n = u32::from_le_bytes(head[..4].try_into().unwrap()) as usize;
        if n == 0 || n > 200_000 {
            return Err(format!("{arc}: {n} entries"));
        }
        let idx = files
            .read_range(arc, 8, n * ENTRY)
            .ok_or(format!("{arc}: short index"))?;
        let key = guess_key(&idx, 8);
        let d: Vec<u8> = idx
            .iter()
            .enumerate()
            .map(|(i, &b)| b.wrapping_sub(key[(8 + i) % key.len()]))
            .collect();
        for e in d.chunks_exact(ENTRY) {
            let name = &e[..32];
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(32)];
            let u = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
            self.files.insert(
                sjis(name).to_lowercase(),
                Entry {
                    arc: arc.to_string(),
                    size: u(32),
                    stored: u(36),
                    offset: u(40),
                },
            );
        }
        Ok(())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.files.get(&name.to_lowercase())?;
        let raw = files.read_range(&e.arc, e.offset as u64, e.stored as usize)?;
        Some(if e.stored == e.size {
            raw
        } else {
            lzss(&raw, e.size as usize)
        })
    }
}

pub fn lzss(src: &[u8], size: usize) -> Vec<u8> {
    let mut win = [0u8; 4096];
    let mut wp = 0xfee;
    let mut out = Vec::with_capacity(size.min(src.len() * 9));
    let mut i = 0;
    while i < src.len() && out.len() < size {
        let flags = src[i];
        i += 1;
        for bit in 0..8 {
            if i >= src.len() || out.len() >= size {
                break;
            }
            if flags >> bit & 1 == 1 {
                let c = src[i];
                i += 1;
                out.push(c);
                win[wp] = c;
                wp = (wp + 1) & 0xfff;
            } else {
                if i + 1 >= src.len() {
                    break;
                }
                let (lo, hi) = (src[i] as usize, src[i + 1] as usize);
                i += 2;
                let p = lo | (hi & 0xf0) << 4;
                for k in 0..(hi & 0xf) + 3 {
                    let c = win[(p + k) & 0xfff];
                    out.push(c);
                    win[wp] = c;
                    wp = (wp + 1) & 0xfff;
                }
            }
        }
    }
    out.truncate(size);
    out
}
