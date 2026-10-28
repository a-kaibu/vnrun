//! MPK archives (GARbro Propeller/ArcMPK.cs, MIT, and BB.exe):
//!
//! ```text
//! index_offset:u32 count:u32
//! at index_offset: count entries of 0x28 bytes, every byte XORed with the last byte of the
//! first name (a NUL): name[0x20] (a leading '\' is dropped) offset:u32 size:u32
//! ```
//!
//! `.msc` entries that start with 0x88 are XORed with 0x88 as a whole.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
}

pub struct Mpk {
    /// the file in the game folder
    pub path: String,
    /// lower-case name to its entry
    pub entries: HashMap<String, Entry>,
}

fn u32_(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

impl Mpk {
    pub fn parse_index(path: &str, mut index: Vec<u8>, count: usize, len: u64) -> Option<Mpk> {
        if index.len() < count * 0x28 || count == 0 {
            return None;
        }
        let key = index[0x1f];
        for b in index.iter_mut() {
            *b ^= key;
        }
        let mut entries = HashMap::new();
        for i in 0..count {
            let e = &index[i * 0x28..(i + 1) * 0x28];
            let start = usize::from(e[0] == b'\\');
            let end = e[..0x20].iter().position(|&c| c == 0).unwrap_or(0x20);
            if end <= start {
                return None;
            }
            let name = String::from_utf8_lossy(&e[start..end]).to_ascii_lowercase();
            let (offset, size) = (u32_(e, 0x20) as u64, u32_(e, 0x24));
            if offset + size as u64 > len {
                return None;
            }
            entries.insert(name, Entry { offset, size });
        }
        Some(Mpk {
            path: path.to_string(),
            entries,
        })
    }

    pub fn open(files: &mut dyn Files, path: &str) -> Option<Mpk> {
        let len = files.size(path)?;
        let head = files.read_range(path, 0, 8)?;
        let (off, count) = (u32_(&head, 0) as u64, u32_(&head, 4) as usize);
        if off < 8 || off >= len || count > 100_000 {
            return None;
        }
        let index = files.read_range(path, off, count * 0x28)?;
        Mpk::parse_index(path, index, count, len)
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let key = name.to_ascii_lowercase();
        let e = self.entries.get(&key)?;
        let mut b = files.read_range(&self.path, e.offset, e.size as usize)?;
        if key.ends_with(".msc") && b.first() == Some(&0x88) {
            for c in b.iter_mut() {
                *c ^= 0x88;
            }
        }
        Some(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index() {
        let mut ix = [0u8; 0x50];
        ix[..9].copy_from_slice(b"\\a.msc\0\0\0");
        ix[0x20] = 0x58;
        ix[0x24] = 4;
        ix[0x28..0x2e].copy_from_slice(b"b.mgr\0");
        ix[0x48] = 0x5c;
        ix[0x4c] = 2;
        let enc: Vec<u8> = ix.iter().map(|b| b ^ 0x58).collect();
        let m = Mpk::parse_index("x.mpk", enc, 2, 0x100).unwrap();
        assert_eq!(m.entries["a.msc"].offset, 0x58);
        assert_eq!(m.entries["b.mgr"].size, 2);
    }
}
