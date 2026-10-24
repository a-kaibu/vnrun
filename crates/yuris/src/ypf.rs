//! YPF archives (ysbin.ypf, cg.ypf, ...): an index of names, then the files, each zlib-compressed
//! or stored.
//!
//!     "YPF\0" version:u32 count:u32 data_start:u32 (16 bytes of zeros)
//!     count x { hash:u32 name_len:u8 name:[u8] kind:u8 compressed:u8 size:u32 packed_size:u32
//!               offset:u32 checksum:u32 }
//!
//! Names are stored with every byte inverted, in Shift_JIS, with `\` between folders. The length
//! byte is inverted too and then mapped through a table of swapped pairs.

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

// Pairs of name lengths the archive swaps (after inverting the byte).
const SWAPS: [(u8, u8); 12] = [
    (0x03, 0x48),
    (0x06, 0x35),
    (0x09, 0x0b),
    (0x0c, 0x10),
    (0x0d, 0x13),
    (0x11, 0x19),
    (0x15, 0x1b),
    (0x1c, 0x1e),
    (0x20, 0x23),
    (0x26, 0x29),
    (0x2c, 0x2f),
    (0x2e, 0x32),
];

fn name_len(stored: u8) -> usize {
    let n = !stored;
    for (a, b) in SWAPS {
        if n == a {
            return b as usize;
        }
        if n == b {
            return a as usize;
        }
    }
    n as usize
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
        if head.get(..4) != Some(b"YPF\0") {
            return Err("not a YPF archive".into());
        }
        let version = u32_at(head, 4).ok_or("short header")?;
        let count = u32_at(head, 8).ok_or("short header")? as usize;
        let data_start = u32_at(head, 12).ok_or("short header")? as usize;
        let mut at = 0x20;
        let mut index = Index { version, ..Default::default() };
        for _ in 0..count {
            let stored = *head.get(at + 4).ok_or("short index")?;
            let len = name_len(stored);
            let raw: Vec<u8> = head.get(at + 5..at + 5 + len).ok_or("short index")?.iter().map(|b| !b).collect();
            let p = at + 5 + len;
            let compressed = *head.get(p + 1).ok_or("short index")? != 0;
            let size = u32_at(head, p + 2).ok_or("short index")?;
            let packed_size = u32_at(head, p + 6).ok_or("short index")?;
            let offset = u32_at(head, p + 10).ok_or("short index")?;
            if (offset as usize) < data_start {
                return Err(format!("entry {} points into the index", index.entries.len()));
            }
            let name = sjis::decode(&raw);
            index.by_name.insert(fold(&name), index.entries.len());
            index.entries.push(Entry { name, compressed, size, packed_size, offset });
            at = p + 18;
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
