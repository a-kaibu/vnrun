//! NonColor's archives (`script.dat`, `arc0.dat`, ...): an index of entries found by the CRC-64
//! of their names, after GARbro's `NonColor/ArcDAT.cs` and `ArcACV.cs` (MIT, by morkt).
//!
//! - `ACV1` (2018): "ACV1", the count xor 0x8B6A4E5F, entries from offset 8;
//!   the older form: the count xor 0x26ACA46E, entries from offset 4.
//! - An entry: u64 hash, u8 flags, u32 offset, size, unpacked size (not used), each xored with the hash's
//!   low bits (the offset of an ACV1 entry also with 0x8B6A4E5F). Without flag 2 the offset and
//!   sizes are also xored with bytes of the name (at lengths / 2, / 4 and / 8).
//! - Flag 2: zlib data whose u32 words are xored with the hash's low half and the game's key;
//!   flag 1: blocks xored with the name's bytes (the data split in name-length parts).
//!
//! The key is not in the files; it is found from the first zlib header of a script.

use std::collections::HashMap;

use nexas::files::Files;

const ACV_KEY: u32 = 0x8B6A_4E5F;
const DAT_KEY: u32 = 0x26AC_A46E;
const ENTRY: usize = 21;

#[derive(Clone, Debug)]
pub struct Entry {
    pub file: String,
    pub hash: u64,
    pub flags: u8,
    offset: u32,
    size: u32,
}

/// CRC-64/WE (GARbro's `Crc64`): polynomial 0x42F0E1EBA9EA3693, not reflected, ~0 in and out.
pub fn crc64(b: &[u8]) -> u64 {
    const POLY: u64 = 0x42F0_E1EB_A9EA_3693;
    let mut c = !0u64;
    for &x in b {
        c ^= (x as u64) << 56;
        for _ in 0..8 {
            c = if c >> 63 != 0 {
                (c << 1) ^ POLY
            } else {
                c << 1
            };
        }
    }
    !c
}

#[derive(Default)]
pub struct Archives {
    entries: HashMap<u64, Entry>,
    /// the game's key for packed entries (the low half of the title's hash)
    pub key: u32,
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

fn xor_words(d: &mut [u8], key: u32) {
    for w in d.chunks_exact_mut(4) {
        let v = u32::from_le_bytes(w.try_into().unwrap()) ^ key;
        w.copy_from_slice(&v.to_le_bytes());
    }
}

/// The unpacked size in the index is not the data's size, so the output is only capped.
fn inflate(d: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(d, 64 << 20).ok()
}

impl Archives {
    /// Every archive at the top of the game folder.
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives::default();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| !n.contains('/') && n.to_lowercase().ends_with(".dat"))
            .collect();
        names.sort();
        for n in names {
            a.index(files, &n);
        }
        a.key = a.find_key(files).unwrap_or(0);
        a
    }

    fn index(&mut self, files: &mut dyn Files, name: &str) {
        let Some(head) = files.read_range(name, 0, 8) else {
            return;
        };
        if head.len() < 8 {
            return;
        }
        let (acv, count, at) = if &head[..4] == b"ACV1" {
            (true, u32_at(&head, 4) ^ ACV_KEY, 8)
        } else {
            (false, u32_at(&head, 0) ^ DAT_KEY, 4)
        };
        let size = files.size(name).unwrap_or(0);
        if count == 0 || count > 200_000 || (at + count as usize * ENTRY) as u64 > size {
            return;
        }
        let Some(idx) = files.read_range(name, at as u64, count as usize * ENTRY) else {
            return;
        };
        for e in idx.chunks_exact(ENTRY) {
            let hash = u64::from_le_bytes(e[..8].try_into().unwrap());
            let lo = hash as u32;
            let e = Entry {
                file: name.to_string(),
                hash,
                flags: e[8] ^ hash as u8,
                offset: u32_at(e, 9) ^ lo ^ if acv { ACV_KEY } else { 0 },
                size: u32_at(e, 13) ^ lo,
            };
            self.entries.entry(hash).or_insert(e);
        }
    }

    /// The key of the packed entries: the zlib header (0x78, then 0x01, 0x5e, 0x9c or 0xda)
    /// gives its low 16 bits, and the rest is tried on the smallest packed entry.
    fn find_key(&self, files: &mut dyn Files) -> Option<u32> {
        let mut packed: Vec<&Entry> = self
            .entries
            .values()
            .filter(|e| e.flags & 2 != 0 && e.size >= 8)
            .collect();
        packed.sort_by_key(|e| (e.size, e.hash));
        let small: Vec<(Vec<u8>, &Entry)> = packed
            .iter()
            .take(3)
            .filter_map(|e| {
                Some((
                    files.read_range(&e.file, e.offset as u64, e.size as usize)?,
                    *e,
                ))
            })
            .collect();
        let (d, e) = small.first()?;
        let first = u32_at(d, 0) ^ e.hash as u32;
        for second in [0x9cu32, 0xda, 0x01, 0x5e] {
            let low = (first ^ (0x78 | second << 8)) & 0xffff;
            for high in 0..0x1_0000u32 {
                let k = low | high << 16;
                let ok = small.iter().all(|(d, e)| {
                    let mut d = d.clone();
                    xor_words(&mut d, e.hash as u32 ^ k);
                    inflate(&d).is_some()
                });
                if ok {
                    return Some(k);
                }
            }
        }
        None
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.get(&crc64(&nexas::sjis::encode(name)))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    /// The entries of one archive file (for the scripts, whose names are not needed).
    pub fn entries_of<'a>(&'a self, file: &'a str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries
            .values()
            .filter(move |e| e.file.eq_ignore_ascii_case(file))
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let raw = nexas::sjis::encode(name);
        let e = self.entry(name)?;
        self.read_entry(files, e, &raw)
    }

    /// An entry's data; `raw` is its name (needed unless it is packed).
    pub fn read_entry(&self, files: &mut dyn Files, e: &Entry, raw: &[u8]) -> Option<Vec<u8>> {
        let (mut offset, mut size) = (e.offset, e.size);
        if e.flags & 2 == 0 {
            if raw.is_empty() {
                return None;
            }
            offset ^= raw[raw.len() >> 1] as u32;
            size ^= raw[raw.len() >> 2] as u32;
        }
        let mut d = files.read_range(&e.file, offset as u64, size as usize)?;
        if e.flags & 2 != 0 {
            xor_words(&mut d, e.hash as u32 ^ self.key);
            return inflate(&d);
        }
        if e.flags & 1 != 0 {
            let block = d.len() / raw.len();
            let mut n = 0;
            for &c in &raw[..raw.len() - 1] {
                for b in &mut d[n..n + block] {
                    *b ^= c;
                }
                n += block;
            }
        }
        Some(d)
    }
}
