//! YKC001 archives: "YKC001", the header size (0x18), then where the index is and its size.
//! The index has 20 bytes a file: where its name is and the name's length (with its 0), where
//! its data is and its size, and a reserved word. Names are Shift_JIS paths with backslashes.

use std::collections::HashMap;

/// The files of all the archives, by lowercase name with forward slashes: (archive, offset, size).
#[derive(Default)]
pub struct Index {
    pub files: HashMap<String, (String, u64, usize)>,
}

/// A path as the index keeps it.
pub fn key(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

impl Index {
    /// Adds an archive given its header and index; earlier archives win.
    pub fn add(&mut self, archive: &str, read: &mut dyn FnMut(u64, usize) -> Option<Vec<u8>>) {
        let Some(h) = read(0, 0x18) else { return };
        if h.len() < 0x18 || !h.starts_with(b"YKC001") {
            return;
        }
        let u = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let (at, size) = (u(&h, 0x10) as u64, u(&h, 0x14) as usize);
        let Some(ix) = read(at, size) else { return };
        let n = ix.len() / 20;
        if n == 0 {
            return;
        }
        // the names lie together (just before the index)
        let first = (0..n).map(|i| u(&ix, i * 20)).min().unwrap_or(0) as u64;
        let last = (0..n)
            .map(|i| u(&ix, i * 20) as u64 + u(&ix, i * 20 + 4) as u64)
            .max()
            .unwrap_or(first);
        let Some(names) = read(first, (last - first) as usize) else {
            return;
        };
        for i in 0..n {
            let e = &ix[i * 20..i * 20 + 20];
            let (no, nl, off, len) = (u(e, 0) as u64, u(e, 4) as usize, u(e, 8), u(e, 12));
            let s = (no - first) as usize;
            let Some(raw) = names.get(s..s + nl) else {
                continue;
            };
            let raw = raw.split(|&c| c == 0).next().unwrap_or(raw);
            let name = key(&nexas::sjis::decode(raw));
            self.files
                .entry(name)
                .or_insert((archive.to_string(), off as u64, len as usize));
        }
    }

    pub fn find(&self, name: &str) -> Option<&(String, u64, usize)> {
        self.files.get(&key(name))
    }
}
