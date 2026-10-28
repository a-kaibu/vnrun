//! AZ System archives (`*.arc`, "ARC\x1a"), their encrypted scripts (`.asb`) and tables
//! (`.tbl`), after GARbro's ArcAZSys.cs.
//!
//! The index after the 0x30 byte header is packed with a small LZ (a control bitmask, 16 bit
//! back references, literal runs) into 0x40 byte entries: offset, size, name at 0x10. An `.asb`
//! is "ASB\x1a", packed size, unpacked size, then 32 bit words less a key (from the game's
//! archive key and the unpacked size), the first of them the CRC32 of the zlib data after it.
//! The key is found by trying every one that turns the data into a zlib header and stream.

use std::cell::Cell;
use std::collections::HashMap;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
use nexas::files::Files;

pub struct Arc {
    path: String,
    /// lowercase name: (file offset, size)
    entries: HashMap<String, (u64, u32)>,
    /// the archive key, once a script has shown it
    key: Cell<Option<u32>>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// The LZ of the index: `out_len` bytes from `p` (control, back references, literals).
fn unpack(p: &[u8], out_len: usize) -> Option<Vec<u8>> {
    let (cl, c1) = (u32_at(p, 4)? as usize, u32_at(p, 8)? as usize);
    let src1 = cl.checked_add(0x14)?;
    let (mut ctl, mut src1, mut src2) = (0x14, src1, src1.checked_add(c1)?);
    let mut out = Vec::with_capacity(out_len);
    let mut mask = 0x80u8;
    while out.len() < out_len {
        if p.get(ctl)? & mask != 0 {
            let v = u16::from_le_bytes(p.get(src1..src1 + 2)?.try_into().ok()?) as usize;
            src1 += 2;
            let (n, off) = ((v >> 13) + 3, (v & 0x1fff) + 1);
            if off > out.len() {
                return None;
            }
            for _ in 0..n {
                out.push(out[out.len() - off]);
            }
        } else {
            let n = *p.get(src2)? as usize + 1;
            out.extend_from_slice(p.get(src2 + 1..src2 + 1 + n)?);
            src2 += 1 + n;
        }
        mask >>= 1;
        if mask == 0 {
            ctl += 1;
            mask = 0x80;
        }
    }
    Some(out)
}

impl Arc {
    /// The first `*.arc` of the game folder that is an AZ archive.
    pub fn open(files: &mut dyn Files) -> Result<Arc, String> {
        let mut names = files.list();
        names.sort();
        for path in names {
            if !path.to_ascii_lowercase().ends_with(".arc") || path.contains('/') {
                continue;
            }
            let Some(h) = files.read_range(&path, 0, 0x30) else {
                continue;
            };
            if !h.starts_with(b"ARC\x1a") {
                continue;
            }
            let count = u32_at(&h, 8).unwrap_or(0) as usize;
            let ilen = u32_at(&h, 12).unwrap_or(0) as usize;
            let p = files
                .read_range(&path, 0x30, ilen)
                .ok_or(format!("{path}: no index"))?;
            let out = unpack(&p, count * 0x40).ok_or(format!("{path}: bad index"))?;
            let base = 0x30 + ilen as u64;
            let mut entries = HashMap::new();
            for e in out.chunks_exact(0x40).take(count) {
                let name = &e[0x10..];
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                let name = nexas::sjis::decode(name).to_lowercase();
                let off = u32_at(e, 0).unwrap_or(0) as u64;
                entries.insert(name, (base + off, u32_at(e, 4).unwrap_or(0)));
            }
            return Ok(Arc {
                path,
                entries,
                key: Cell::new(None),
            });
        }
        Err("no AZ System archive (*.arc)".into())
    }

    pub fn exists(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let &(off, size) = self.entries.get(&name.to_lowercase())?;
        files.read_range(&self.path, off, size as usize)
    }

    /// A script, decrypted and unpacked.
    pub fn asb(&self, files: &mut dyn Files, name: &str) -> Result<Vec<u8>, String> {
        let d = self.read(files, name).ok_or(format!("{name}: missing"))?;
        if !d.starts_with(b"ASB\x1a") {
            return Err(format!("{name}: not an ASB"));
        }
        let packed = u32_at(&d, 4).unwrap_or(0) as usize;
        let unpacked = u32_at(&d, 8).unwrap_or(0);
        let inp = d.get(12..12 + packed).ok_or(format!("{name}: short"))?;
        let words: Vec<u32> = inp
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let full = |k: u32| -> Vec<u8> {
            let mut out: Vec<u8> = words
                .iter()
                .flat_map(|w| w.wrapping_sub(k).to_le_bytes())
                .collect();
            out.extend_from_slice(&inp[words.len() * 4..]);
            out
        };
        let checked = |b: &[u8]| b.len() > 6 && u32_at(b, 0) == Some(crc32fast::hash(&b[4..]));
        let mut found = None;
        if let Some(ak) = self.key.get() {
            let k = entry_key(ak ^ unpacked);
            let b = full(k);
            if checked(&b) {
                found = Some(b);
            }
        }
        if found.is_none() {
            let k = find_key(&words, |k| checked(&full(k))).ok_or(format!("{name}: no key"))?;
            let b = full(k);
            self.key.set(Some(archive_key(k) ^ unpacked));
            found = Some(b);
        }
        let b = found.unwrap();
        miniz_oxide::inflate::decompress_to_vec_zlib(&b[4..]).map_err(|e| format!("{name}: {e:?}"))
    }

    /// A table: rows of cells (a plain "TBL\x1a" is one row).
    pub fn tbl(&self, files: &mut dyn Files, name: &str) -> Result<Vec<Vec<String>>, String> {
        let d = self.read(files, name).ok_or(format!("{name}: missing"))?;
        tbl(&d).ok_or(format!("{name}: bad table"))
    }
}

/// The key of one script from the archive key xor its unpacked size.
fn entry_key(k: u32) -> u32 {
    k ^ ((k << 12) | k) << 11
}

/// Inverts `entry_key`, a bit at a time from the bottom.
fn archive_key(kp: u32) -> u32 {
    let mut k = 0u32;
    for i in 0..32 {
        let mut t = 0;
        if i >= 23 {
            t |= (k >> (i - 23)) & 1;
        }
        if i >= 11 {
            t |= (k >> (i - 11)) & 1;
        }
        k |= (((kp >> i) & 1) ^ t) << i;
    }
    k
}

/// A key that makes the words after the CRC the start of a zlib stream, and passes `check`.
fn find_key(words: &[u32], check: impl Fn(u32) -> bool) -> Option<u32> {
    let w1 = *words.get(1)?;
    let n = words.len().min(48);
    let mut st = Box::<DecompressorOxide>::default();
    let mut out = vec![0u8; 1 << 15];
    for head in [0xda78u32, 0x9c78, 0x0178, 0x5e78] {
        let lo = w1.wrapping_sub(head) & 0xffff;
        for hi in 0..0x10000u32 {
            let k = lo | hi << 16;
            let b: Vec<u8> = words[1..n]
                .iter()
                .flat_map(|w| w.wrapping_sub(k).to_le_bytes())
                .collect();
            st.init();
            let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
                | inflate_flags::TINFL_FLAG_HAS_MORE_INPUT
                | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
            let (status, _, _) = decompress(&mut st, &b, &mut out, 0, flags);
            if matches!(
                status,
                TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput | TINFLStatus::Done
            ) && check(k)
            {
                return Some(k);
            }
        }
    }
    None
}

/// "TBL\x1a", the size from the next field on, the unpacked size, a CRC32 and zlib data: 48
/// byte cells, a row. "TBL2" (rows, columns) is followed by its rows like that.
pub fn tbl(d: &[u8]) -> Option<Vec<Vec<String>>> {
    let cells = |z: &[u8]| -> Option<Vec<String>> {
        let u = miniz_oxide::inflate::decompress_to_vec_zlib(z).ok()?;
        Some(
            u.chunks(48)
                .map(|c| {
                    let c = &c[..c.iter().position(|&b| b == 0).unwrap_or(c.len())];
                    nexas::sjis::decode(c)
                })
                .collect(),
        )
    };
    if d.starts_with(b"TBL\x1a") {
        let end = (u32_at(d, 4)? as usize).checked_add(12)?;
        return Some(vec![cells(d.get(16..end)?)?]);
    }
    if !d.starts_with(b"TBL2") {
        return None;
    }
    let mut rows = Vec::new();
    let mut p = 12;
    while d.get(p..p + 4) == Some(b"TBL\x1a") {
        let end = (u32_at(d, p + 4)? as usize).checked_add(p + 12)?;
        rows.push(cells(d.get(p + 16..end)?)?);
        p = end;
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_invert() {
        for k in [0xf389842du32, 1, 0xffff_ffff, 0x1234_5678] {
            assert_eq!(archive_key(entry_key(k)), k);
        }
    }
}
