//! MajiroArcV1/2/3.000 archives: a header, a table of (hash, offset[, size]) entries, the
//! NUL separated names in the same order, then the data.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    pub size: usize,
}

pub struct Arc {
    pub file: String,
    pub entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
}

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

impl Arc {
    pub fn open(files: &mut dyn Files, file: &str) -> Result<Arc, String> {
        let head = files.read_range(file, 0, 0x1c).ok_or("short file")?;
        if &head[..10] != b"MajiroArcV" || &head[11..16] != b".000\0" {
            return Err(format!("{file}: not a Majiro archive"));
        }
        let version = head[10].wrapping_sub(b'0') as usize;
        if !(1..=3).contains(&version) {
            return Err(format!("{file}: version {version}"));
        }
        let count = u32le(&head, 16) as usize;
        let names_at = u32le(&head, 20) as usize;
        let data_at = u32le(&head, 24) as usize;
        if names_at < 0x1c || data_at < names_at {
            return Err(format!("{file}: bad index"));
        }
        let index = files
            .read_range(file, 0x1c, data_at - 0x1c)
            .ok_or("short index")?;
        let (table, names) = index.split_at(names_at - 0x1c);
        let entry_size = 4 * (version + 1);
        let hash_size = if version < 3 { 4 } else { 8 };
        let mut entries = Vec::with_capacity(count);
        let mut by_name = HashMap::new();
        for (i, raw) in names.split(|&b| b == 0).take(count).enumerate() {
            let at = i * entry_size + hash_size;
            if at + 4 > table.len() {
                break;
            }
            let offset = u32le(table, at) as u64;
            let size = if version == 1 {
                (u32le(table, at + entry_size) as u64).saturating_sub(offset) as usize
            } else {
                u32le(table, at + 4) as usize
            };
            let name = encoding_rs::SHIFT_JIS.decode(raw).0.into_owned();
            by_name.insert(name.to_lowercase(), entries.len());
            entries.push(Entry { name, offset, size });
        }
        Ok(Arc {
            file: file.to_string(),
            entries,
            by_name,
        })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name
            .get(&name.to_lowercase())
            .map(|&i| &self.entries[i])
    }

    pub fn read(&self, files: &mut dyn Files, e: &Entry) -> Option<Vec<u8>> {
        files.read_range(&self.file, e.offset, e.size)
    }
}
