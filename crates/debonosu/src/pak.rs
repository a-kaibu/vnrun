//! `.pak` archives: "PAK\0", u16 0x10, u16 6, u32 1, u32 0, u64 0x18; at 0x18 u32 count,
//! u32 directory size, u64 stored directory size; the directory (raw deflate) follows at 0x28.
//! Each directory entry: u64 offset, u64 size, u64 stored size, u32 attributes (0x10 a folder,
//! 0x20 a file), three FILETIMEs and a Shift-JIS name ending in 0. A file's data is at
//! 0x28 + stored directory size + offset, raw deflate when the stored size differs.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub pak: String,
    pub offset: u64,
    pub size: u64,
    pub stored: u64,
}

/// The files of all archives by lower-case name without folder, and by name without extension
/// (a later archive replaces an earlier one).
#[derive(Default)]
pub struct Index {
    pub files: HashMap<String, Entry>,
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

pub fn inflate(b: &[u8], size: usize) -> Option<Vec<u8>> {
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(b, size.max(1) * 2 + 64).ok()?;
    Some(out)
}

impl Index {
    pub fn add(&mut self, files: &mut dyn Files, pak: &str) -> Result<usize, String> {
        let head = files.read_range(pak, 0, 0x28).ok_or("short header")?;
        if &head[0..4] != b"PAK\0" {
            return Err(format!("{pak}: not a PAK"));
        }
        let dir_size = u32le(&head, 0x1c) as usize;
        let dir_stored = u64le(&head, 0x20);
        let raw = files
            .read_range(pak, 0x28, dir_stored as usize)
            .ok_or("short directory")?;
        let dir = inflate(&raw, dir_size).ok_or("bad directory")?;
        let base = 0x28 + dir_stored;
        let mut p = 0;
        let mut n = 0;
        while p + 52 <= dir.len() {
            let (offset, size, stored) = (u64le(&dir, p), u64le(&dir, p + 8), u64le(&dir, p + 16));
            let attr = u32le(&dir, p + 24);
            p += 52;
            let end = dir[p..]
                .iter()
                .position(|&c| c == 0)
                .map_or(dir.len(), |e| p + e);
            let name = nexas::sjis::decode(&dir[p..end]).to_lowercase();
            p = end + 1;
            if attr & 0x20 == 0 {
                continue;
            }
            let e = Entry {
                pak: pak.to_string(),
                offset: base + offset,
                size,
                stored,
            };
            if let Some((stem, _)) = name.rsplit_once('.') {
                self.files
                    .entry(stem.to_string())
                    .or_insert_with(|| e.clone());
            }
            self.files.insert(name, e);
            n += 1;
        }
        Ok(n)
    }

    pub fn has(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.files.get(&name.to_lowercase())?;
        let raw = files.read_range(&e.pak, e.offset, e.stored as usize)?;
        if e.stored == e.size {
            Some(raw)
        } else {
            inflate(&raw, e.size as usize)
        }
    }
}
