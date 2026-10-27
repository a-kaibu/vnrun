//! .mbl archives of the Marble engine (the 2006–2008 Lune and Marine games, before the
//! Tmr-Hiro ADV System): a flat list of named files.
//!
//! ```text
//! count:u32 name_len:u32 count x { name:[u8; name_len] offset:u32 size:u32 }   (newer)
//! count:u32              count x { name:[u8; 16]       offset:u32 size:u32 }   (older)
//! ```
//!
//! A name field holds the name, a NUL, then the extension ("PRS", "S", "OGG", or nothing).
//! Scripts (".S") are stored with every byte negated. The layout follows GARbro's
//! ArcFormats/Marble (MIT).

use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Default)]
pub struct Index {
    /// by upper-case name, without the extension
    pub entries: HashMap<String, Entry>,
    pub names: Vec<String>,
}

impl Index {
    /// How many bytes of the archive's start to read to parse the index, from its first 8.
    pub fn index_size(head: &[u8]) -> Option<usize> {
        let u = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().unwrap()) as usize;
        if head.len() < 8 {
            return None;
        }
        let (count, name_len) = (u(0), u(4));
        if (1..=0x40).contains(&name_len) {
            Some(8 + count * (name_len + 8))
        } else {
            Some(4 + count * 24)
        }
    }

    pub fn parse(b: &[u8], file_size: u64) -> Result<Index, String> {
        let u = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        if b.len() < 8 {
            return Err("mbl: too short".into());
        }
        let count = u(0) as usize;
        let try_layout = |name_len: usize, start: usize| -> Option<Index> {
            if start + count * (name_len + 8) > b.len() {
                return None;
            }
            let mut index = Index::default();
            let mut at = start;
            for _ in 0..count {
                let field = &b[at..at + name_len];
                let end = field.iter().position(|&c| c == 0).unwrap_or(name_len);
                let name = nexas::sjis::decode(&field[..end]).to_uppercase();
                let e = Entry {
                    offset: u(at + name_len),
                    size: u(at + name_len + 4),
                };
                if name.is_empty() || e.offset as u64 + e.size as u64 > file_size {
                    return None;
                }
                index.names.push(name.clone());
                index.entries.insert(name, e);
                at += name_len + 8;
            }
            Some(index)
        };
        let name_len = u(4) as usize;
        (1..=0x40)
            .contains(&name_len)
            .then(|| try_layout(name_len, 8))
            .flatten()
            .or_else(|| try_layout(16, 4))
            .ok_or_else(|| "mbl: not a Marble archive".into())
    }
}

/// A script as stored: every byte negated.
pub fn script(b: &[u8]) -> Vec<u8> {
    b.iter().map(|c| c.wrapping_neg()).collect()
}
