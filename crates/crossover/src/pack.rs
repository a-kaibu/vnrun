//! The `PACK` archives of DATA/ (SCRIPT.PAC, *.GR2, *.VIC), as read by Adv.exe at 0x407580:
//!
//! ```text
//! "PACK" count:u32 data_start:u32 0:u32
//! count entries of 24 bytes from 16: name ([u8; 16], NUL padded) size:u32 offset:u32
//! ```
//!
//! Offsets are from the start of the file. The game asks for entries by their place (from 1)
//! or, for voices, by name.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    pub size: u32,
}

pub struct Pack {
    /// the file in the game folder
    pub path: String,
    pub entries: Vec<Entry>,
    /// upper-case name to its place in `entries`
    by_name: HashMap<String, usize>,
}

fn u32_(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

impl Pack {
    pub fn parse_index(path: &str, head: &[u8], len: u64) -> Option<Pack> {
        if head.len() < 16 || &head[..4] != b"PACK" {
            return None;
        }
        let count = u32_(head, 4) as usize;
        if count > 100_000 || head.len() < 16 + count * 24 {
            return None;
        }
        let mut p = Pack {
            path: path.to_string(),
            entries: Vec::with_capacity(count),
            by_name: HashMap::new(),
        };
        for i in 0..count {
            let o = 16 + i * 24;
            let raw = &head[o..o + 16];
            let end = raw.iter().position(|&c| c == 0).unwrap_or(16);
            let name = String::from_utf8_lossy(&raw[..end]).to_ascii_uppercase();
            let (size, offset) = (u32_(head, o + 16), u32_(head, o + 20) as u64);
            if offset + size as u64 > len {
                return None;
            }
            p.by_name.insert(name.clone(), i);
            p.entries.push(Entry { name, offset, size });
        }
        Some(p)
    }

    pub fn open(files: &mut dyn Files, path: &str) -> Option<Pack> {
        let len = files.size(path)?;
        let head = files.read_range(path, 0, 16)?;
        if head.len() < 16 || &head[..4] != b"PACK" {
            return None;
        }
        let count = u32_(&head, 4) as u64;
        if count > 100_000 {
            return None;
        }
        let index = files.read_range(path, 0, (16 + count * 24) as usize)?;
        Pack::parse_index(path, &index, len)
    }

    /// The entry at `n` (from 1).
    pub fn read_nth(&self, files: &mut dyn Files, n: usize) -> Option<Vec<u8>> {
        let e = self.entries.get(n.checked_sub(1)?)?;
        files.read_range(&self.path, e.offset, e.size as usize)
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_ascii_uppercase()).map(|i| i + 1)
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let n = self.find(name)?;
        self.read_nth(files, n)
    }
}

#[cfg(test)]
mod tests {
    use super::Pack;

    #[test]
    fn index() {
        let mut b = b"PACK".to_vec();
        b.extend(1u32.to_le_bytes());
        b.extend(40u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        let mut name = *b"A.SCD\0\0\0\0\0\0\0\0\0\0\0";
        name[0] = b'a';
        b.extend(name);
        b.extend(3u32.to_le_bytes());
        b.extend(40u32.to_le_bytes());
        b.extend(b"xyz");
        let p = Pack::parse_index("x", &b, b.len() as u64).unwrap();
        assert_eq!(p.find("a.scd"), Some(1));
        assert_eq!(p.entries[0].offset, 40);
    }
}
