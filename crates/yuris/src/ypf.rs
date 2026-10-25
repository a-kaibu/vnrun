//! YPF archives (ysbin.ypf, cg.ypf, ...): an index of names, then the files, each zlib-compressed
//! or stored.
//!
//! ```text
//! "YPF\0" version:u32 count:u32 data_start:u32 (16 bytes of zeros)
//! count x { hash:u32 name_len:u8 name:[u8] kind:u8 compressed:u8 size:u32 packed_size:u32
//!           offset:u32 checksum:u32 }
//! ```
//!
//! Later versions (482 has it, 466 not) store the offset as a u64.
//!
//! Names are stored in Shift_JIS, with `\` between folders, and every byte XORed with a key: 0xff
//! (inverted) in most games, others (0xc9 in 鯨神のティアスティラ). The key is taken from the
//! first name, whose fourth byte from the end is the `.` of its extension. The length byte is
//! inverted and then mapped through a table of swapped pairs.

use std::collections::HashMap;

use crate::sjis;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub compressed: bool,
    pub size: u32,
    pub packed_size: u32,
    pub offset: u32,
}

#[derive(Debug, Default)]
pub struct Index {
    pub version: u32,
    pub entries: Vec<Entry>,
    /// lower-cased name with `/` between folders -> entry
    by_name: HashMap<String, usize>,
}

// Pairs of name lengths the archive swaps (after inverting the byte). Releases differ (the
// tables of GARbro's ArcYPF.cs): the archive is read with each until one fits.
const SWAPS: [&[u8]; 4] = [
    &[
        0x03, 0x48, 0x06, 0x35, 0x0c, 0x10, 0x11, 0x19, 0x1c, 0x1e, 0x09, 0x0b, 0x0d, 0x13, 0x15, 0x1b, 0x20, 0x23,
        0x26, 0x29, 0x2c, 0x2f, 0x2e, 0x32,
    ],
    // Whirlpool (鯨神のティアスティラ, Unionism Quartet, ...)
    &[
        0x03, 0x0a, 0x06, 0x35, 0x0c, 0x10, 0x11, 0x18, 0x1c, 0x1e, 0x09, 0x0b, 0x0d, 0x13, 0x15, 0x1b, 0x20, 0x23,
        0x26, 0x29, 0x2c, 0x2f, 0x14, 0x2e,
    ],
    &[
        0x0c, 0x10, 0x11, 0x19, 0x1c, 0x1e, 0x09, 0x0b, 0x0d, 0x13, 0x15, 0x1b, 0x20, 0x23, 0x26, 0x29, 0x2c, 0x2f,
        0x2e, 0x32,
    ],
    &[0x09, 0x0b, 0x0d, 0x13, 0x15, 0x1b, 0x20, 0x23, 0x26, 0x29, 0x2c, 0x2f, 0x2e, 0x32],
];

fn name_len(swaps: &[u8], stored: u8) -> usize {
    let n = !stored;
    match swaps.iter().position(|&b| b == n) {
        Some(i) => swaps[i ^ 1] as usize,
        None => n as usize,
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Folds a name for lookup: lower case, `/` between folders.
pub fn fold(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

impl Index {
    /// Reads the index from the start of an archive (the first `data_start` bytes are enough).
    pub fn parse(head: &[u8]) -> Result<Index, String> {
        // Preferably the reading whose index ends where the files start.
        let mut first = None;
        for exact in [true, false] {
            for swaps in SWAPS {
                for wide_offsets in [false, true] {
                    match Index::parse_with(head, swaps, wide_offsets, exact) {
                        Ok(index) => return Ok(index),
                        Err(e) => drop(first.get_or_insert(e)),
                    }
                }
            }
        }
        Err(first.unwrap_or_default())
    }

    fn parse_with(head: &[u8], swaps: &[u8], wide_offsets: bool, exact: bool) -> Result<Index, String> {
        if head.get(..4) != Some(b"YPF\0") {
            return Err("not a YPF archive".into());
        }
        let version = u32_at(head, 4).ok_or("short header")?;
        let count = u32_at(head, 8).ok_or("short header")? as usize;
        let data_start = u32_at(head, 12).ok_or("short header")? as usize;
        let mut at = 0x20;
        let mut index = Index { version, ..Default::default() };
        let mut key = None;
        for _ in 0..count {
            let stored = *head.get(at + 4).ok_or("short index")?;
            let len = name_len(swaps, stored);
            let stored_name = head.get(at + 5..at + 5 + len).ok_or("short index")?;
            let key = *key.get_or_insert(match len {
                4.. => stored_name[len - 4] ^ b'.',
                _ => 0xff,
            });
            let raw: Vec<u8> = stored_name.iter().map(|b| b ^ key).collect();
            let p = at + 5 + len;
            let compressed = *head.get(p + 1).ok_or("short index")? != 0;
            let size = u32_at(head, p + 2).ok_or("short index")?;
            let packed_size = u32_at(head, p + 6).ok_or("short index")?;
            let offset = u32_at(head, p + 10).ok_or("short index")?;
            if (offset as usize) < data_start || wide_offsets && u32_at(head, p + 14).ok_or("short index")? != 0 {
                return Err(format!("entry {} points into the index", index.entries.len()));
            }
            let name = sjis::decode(&raw);
            index.by_name.insert(fold(&name), index.entries.len());
            index.entries.push(Entry { name, compressed, size, packed_size, offset });
            at = p + if wide_offsets { 22 } else { 18 };
        }
        if exact && at != data_start {
            return Err("the index does not end where the files start".into());
        }
        Ok(index)
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(&fold(name)).map(|&i| &self.entries[i])
    }
}

impl Entry {
    /// The file's contents from its stored bytes (`packed_size` bytes at `offset`).
    pub fn unpack(&self, stored: &[u8]) -> Result<Vec<u8>, String> {
        if !self.compressed {
            return Ok(stored.to_vec());
        }
        let out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stored, self.size as usize)
            .map_err(|e| format!("{}: {e:?}", self.name))?;
        Ok(out)
    }
}
