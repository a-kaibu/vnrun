//! PAC archives (Script.pac, Visual.pac, ...).
//!
//! ```text
//! "PAC" ver:u8 count:u32 pack:u32 (file data ...) index:[u8; n] n:u32
//! ```
//!
//! The index is at the end: its bytes are inverted and Huffman-coded (the tree first: a 1 bit is
//! a node with two subtrees, a 0 bit a leaf with an 8-bit byte; bits are read from the top of each
//! byte). It decodes to `count` entries of 0x4C bytes: a Shift_JIS name (64 bytes, NUL padded),
//! offset, size, packed size. Pack type 3 and 4 compress files with zlib, 7 (2019 on) with
//! zstd; 4 and 7 store files whose packed size equals their size.

use std::collections::HashMap;

use crate::sjis;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub packed_size: u32,
}

#[derive(Debug, Default)]
pub struct Index {
    pub pack: u32,
    pub entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
}

/// Folds a name for lookup: lower case, `/` between folders.
pub fn fold(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

struct Bits<'a> {
    b: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self.b.get(self.at >> 3).ok_or("PAC index is cut short")?;
        let v = (byte >> (7 - (self.at & 7))) & 1;
        self.at += 1;
        Ok(v as u32)
    }
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        (0..n).try_fold(0, |v, _| Ok(v << 1 | self.bit()?))
    }
}

/// Decodes the Huffman-coded index into `n` bytes.
fn huffman(src: &[u8], n: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits { b: src, at: 0 };
    // nodes 256.. have children; 0..=255 are leaves
    let mut kids: Vec<[u32; 2]> = Vec::new();
    fn tree(bits: &mut Bits, kids: &mut Vec<[u32; 2]>, depth: u32) -> Result<u32, String> {
        if depth > 512 {
            return Err("PAC index tree is too deep".into());
        }
        if bits.bit()? == 1 {
            let i = kids.len();
            kids.push([0, 0]);
            let l = tree(bits, kids, depth + 1)?;
            let r = tree(bits, kids, depth + 1)?;
            kids[i] = [l, r];
            Ok(256 + i as u32)
        } else {
            bits.bits(8)
        }
    }
    let root = tree(&mut bits, &mut kids, 0)?;
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let mut s = root;
        while s >= 256 {
            s = kids[(s - 256) as usize][bits.bit()? as usize];
        }
        out.push(s as u8);
    }
    Ok(out)
}

impl Index {
    /// Reads the index from the whole archive's head (12 bytes) and its tail (`tail` ends at the
    /// end of the file and holds at least the index and its size).
    pub fn parse(head: &[u8], tail: &[u8]) -> Result<Index, String> {
        if head.get(..3) != Some(b"PAC") {
            return Err("not a PAC archive".into());
        }
        let count = u32_at(head, 4).ok_or("short PAC header")? as usize;
        let pack = u32_at(head, 8).ok_or("short PAC header")?;
        let size = u32_at(tail, tail.len().saturating_sub(4)).ok_or("short PAC")? as usize;
        let start = tail
            .len()
            .checked_sub(4 + size)
            .ok_or("PAC index is larger than what was read")?;
        let src: Vec<u8> = tail[start..tail.len() - 4].iter().map(|b| !b).collect();
        let idx = huffman(&src, count * 0x4c)?;
        let mut entries = Vec::with_capacity(count);
        let mut by_name = HashMap::new();
        for i in 0..count {
            let e = &idx[i * 0x4c..i * 0x4c + 0x4c];
            let end = e[..0x40].iter().position(|&b| b == 0).unwrap_or(0x40);
            let name = sjis::decode(&e[..end]);
            by_name.insert(fold(&name), i);
            entries.push(Entry {
                name,
                offset: u32_at(e, 0x40).unwrap(),
                size: u32_at(e, 0x44).unwrap(),
                packed_size: u32_at(e, 0x48).unwrap(),
            });
        }
        Ok(Index {
            pack,
            entries,
            by_name,
        })
    }

    /// The size of the index at the end of an archive, from its last 4 bytes.
    pub fn tail_size(last4: &[u8]) -> Option<usize> {
        Some(u32_at(last4, 0)? as usize + 4)
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(&fold(name)).map(|&i| &self.entries[i])
    }

    /// A file's bytes from its stored bytes.
    pub fn unpack(&self, e: &Entry, raw: &[u8]) -> Result<Vec<u8>, String> {
        let packed = matches!(self.pack, 3 | 4 | 7)
            && e.size != 0
            && !(self.pack != 3 && e.size == e.packed_size);
        if !packed {
            return Ok(raw.to_vec());
        }
        if self.pack == 7 {
            let mut out = Vec::with_capacity(e.size as usize);
            let mut d = ruzstd::decoding::StreamingDecoder::new(raw)
                .map_err(|err| format!("{}: {err}", e.name))?;
            std::io::Read::read_to_end(&mut d, &mut out)
                .map_err(|err| format!("{}: {err}", e.name))?;
            return Ok(out);
        }
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, e.size as usize + 16)
            .map_err(|err| format!("{}: {err:?}", e.name))
    }
}
