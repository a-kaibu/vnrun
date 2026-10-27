//! .pac archives: a flat list of named files.
//!
//! ```text
//! count:u16 name_len:u8 data_offset:u32
//! count x { name:[u8; name_len] offset:u32 size:u32 }             (older)
//! count x { name:[u8; name_len] offset:u64 size:u32 }             (newer)
//! files, `offset` counted from `data_offset`
//! ```

use std::collections::HashMap;

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

impl Index {
    /// How many bytes of the archive's start hold the index, from its first 7 bytes.
    pub fn index_size(head: &[u8]) -> Option<usize> {
        (head.len() >= 7).then(|| u32::from_le_bytes(head[3..7].try_into().unwrap()) as usize)
    }

    pub fn parse(b: &[u8]) -> Result<Index, String> {
        if b.len() < 7 {
            return Err("pac: too short".into());
        }
        let count = u16::from_le_bytes([b[0], b[1]]) as usize;
        let name_len = b[2] as usize;
        let data = u32::from_le_bytes(b[3..7].try_into().unwrap()) as u64;
        let wide = if data == 7 + (name_len as u64 + 8) * count as u64 {
            false
        } else if data == 7 + (name_len as u64 + 12) * count as u64 {
            true
        } else {
            return Err("pac: not a Tmr-Hiro archive".into());
        };
        if name_len == 0 || b.len() < data as usize {
            return Err("pac: index is cut short".into());
        }
        let mut index = Index::default();
        let mut at = 7;
        for _ in 0..count {
            let name = &b[at..at + name_len];
            let name =
                nexas::sjis::decode(&name[..name.iter().position(|&c| c == 0).unwrap_or(name_len)]);
            at += name_len;
            let (offset, size) = if wide {
                let o = u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
                (
                    o,
                    u32::from_le_bytes(b[at + 8..at + 12].try_into().unwrap()),
                )
            } else {
                let o = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as u64;
                (o, u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()))
            };
            at += if wide { 12 } else { 8 };
            index.names.push(name.clone());
            index.entries.insert(
                name.to_lowercase(),
                Entry {
                    offset: data + offset,
                    size,
                },
            );
        }
        Ok(index)
    }

    pub fn find(&self, name: &str) -> Option<Entry> {
        self.entries.get(&name.to_lowercase()).copied()
    }
}
