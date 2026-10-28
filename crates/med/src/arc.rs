//! The archives of the game folder: `md_scr.med`, `md_gra.med`, `md_bgm.med` (MDN), `md_cv.med`
//! and `md_se.med` (DxLib DXA version 4, XORed with a 12-byte key). Both follow GARbro (MIT, by
//! morkt: `DxLib/ArcMED.cs`, `DxLib/ArcDX.cs`).
//!
//! ```text
//! MDN:  "MD" ? ? entry_len:u16 count:u16 ?:[u8; 8], then from 0x10 per entry:
//!       name ([u8; entry_len - 8], NUL padded) size:u32 offset:u32
//! DXA4: "DX" 4:u16 index_size:u32 base:u32 index_offset:u32 file_table:u32 dir_table:u32 ..
//!       every byte XORed with key[offset % 12]; the key is found from the known header
//! ```
//!
//! Entries are found by name without the extension, upper or lower case alike.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
    /// DXA: the unpacked size when the entry is packed
    pub unpacked: Option<u32>,
}

pub struct Pack {
    /// the file in the game folder
    pub path: String,
    /// by upper-case name without extension
    pub entries: HashMap<String, Entry>,
    /// the names in index order (with extensions)
    pub names: Vec<String>,
    /// DXA: the XOR key
    key: Option<[u8; 12]>,
}

fn key_of(name: &str) -> String {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((s, e)) if e.len() <= 4 && !s.is_empty() => s,
        _ => base,
    };
    stem.to_uppercase()
}

fn u16_(b: &[u8], at: usize) -> u32 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u32
}

fn u32_(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn xor(data: &mut [u8], offset: u64, key: &[u8; 12]) {
    let mut k = (offset % 12) as usize;
    for b in data {
        *b ^= key[k];
        k = if k == 11 { 0 } else { k + 1 };
    }
}

impl Pack {
    pub fn open(files: &mut dyn Files, path: &str) -> Option<Pack> {
        let len = files.size(path)?;
        let head = files.read_range(path, 0, 0x30.min(len as usize))?;
        if head.len() < 0x1c {
            return None;
        }
        if &head[..2] == b"MD" {
            return Pack::open_mdn(files, path, &head, len);
        }
        Pack::open_dxa(files, path, &head, len)
    }

    fn open_mdn(files: &mut dyn Files, path: &str, head: &[u8], len: u64) -> Option<Pack> {
        let (entry_len, count) = (u16_(head, 4) as usize, u16_(head, 6) as usize);
        if entry_len <= 8 || count == 0 {
            return None;
        }
        let index = files.read_range(path, 0x10, entry_len * count)?;
        let (mut entries, mut names) = (HashMap::new(), Vec::new());
        for e in index.chunks_exact(entry_len) {
            let raw = &e[..entry_len - 8];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let name = encoding_rs::SHIFT_JIS.decode(&raw[..end]).0.into_owned();
            let (size, offset) = (u32_(e, entry_len - 8), u32_(e, entry_len - 4));
            if offset as u64 + size as u64 > len {
                return None;
            }
            entries.insert(
                key_of(&name),
                Entry {
                    offset: offset as u64,
                    size,
                    unpacked: None,
                },
            );
            names.push(name);
        }
        Some(Pack {
            path: path.to_string(),
            entries,
            names,
            key: None,
        })
    }

    /// DXA version 4: the key is the header XORed with what it must hold ("DX", 4, the base
    /// offset 0x1C, the index size that runs to the end of the file).
    fn open_dxa(files: &mut dyn Files, path: &str, head: &[u8], len: u64) -> Option<Pack> {
        let mut key: [u8; 12] = head[..12].try_into().ok()?;
        key[0] ^= b'D';
        key[1] ^= b'X';
        key[2] ^= 4;
        key[8] ^= 0x1c;
        let index_offset = (u32_(head, 12) ^ u32_(&key, 0)) as u64;
        if index_offset <= 0x1c || index_offset >= len {
            return None;
        }
        let index_size = (len - index_offset) as u32;
        if index_size > 0xff_ffff {
            return None;
        }
        key[4] ^= index_size as u8;
        key[5] ^= (index_size >> 8) as u8;
        key[6] ^= (index_size >> 16) as u8;
        let mut h = head[4..0x1c].to_vec();
        xor(&mut h, 4, &key);
        if u32_(&h, 0) != index_size || u32_(&h, 8) as u64 != index_offset {
            return None;
        }
        let (base, files_at, dirs_at) = (u32_(&h, 4) as u64, u32_(&h, 12), u32_(&h, 16));
        let mut index = files.read_range(path, index_offset, index_size as usize)?;
        xor(&mut index, index_offset, &key);
        let mut pack = Pack {
            path: path.to_string(),
            entries: HashMap::new(),
            names: Vec::new(),
            key: Some(key),
        };
        pack.dxa_dir(&index, files_at, dirs_at, 0, "", base, 0)?;
        Some(pack)
    }

    /// One directory of a DXA index: a dir entry (u32 dir offset, parent, file count, file
    /// table offset) and its file entries of 0x2C bytes (name offset, attributes, 3 times,
    /// data offset, size, packed size or -1).
    #[allow(clippy::too_many_arguments)]
    fn dxa_dir(
        &mut self,
        index: &[u8],
        files_at: u32,
        dirs_at: u32,
        dir: u32,
        root: &str,
        base: u64,
        depth: u32,
    ) -> Option<()> {
        if depth > 16 {
            return None;
        }
        let at = (dirs_at + dir) as usize;
        if at + 16 > index.len() {
            return None;
        }
        let (count, table) = (u32_(index, at + 8), u32_(index, at + 12));
        let name_at = |off: u32| -> Option<String> {
            // names are at the start of the index
            let p = off as usize;
            if p + 4 > index.len() {
                return None;
            }
            let n = u16_(index, p) as usize * 4;
            let s = p + 4 + n;
            let raw = index.get(s..)?;
            let end = raw.iter().position(|&b| b == 0)?;
            Some(encoding_rs::SHIFT_JIS.decode(&raw[..end]).0.into_owned())
        };
        for i in 0..count {
            let e = (files_at + table + i * 0x2c) as usize;
            if e + 0x2c > index.len() {
                return None;
            }
            let name = name_at(u32_(index, e))?;
            let attr = u32_(index, e + 4);
            let offset = u32_(index, e + 0x20);
            let path = if root.is_empty() {
                name
            } else {
                format!("{root}\\{name}")
            };
            if attr & 0x10 != 0 {
                if offset == 0 || offset == dir {
                    return None;
                }
                self.dxa_dir(index, files_at, dirs_at, offset, &path, base, depth + 1)?;
            } else {
                let size = u32_(index, e + 0x24);
                let packed = u32_(index, e + 0x28);
                let entry = if packed == u32::MAX {
                    Entry {
                        offset: base + offset as u64,
                        size,
                        unpacked: None,
                    }
                } else {
                    Entry {
                        offset: base + offset as u64,
                        size: packed,
                        unpacked: Some(size),
                    }
                };
                self.entries.insert(key_of(&path), entry);
                self.names.push(path);
            }
        }
        Some(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&key_of(name))
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&key_of(name))?;
        let mut b = files.read_range(&self.path, e.offset, e.size as usize)?;
        if let Some(key) = &self.key {
            xor(&mut b, e.offset, key);
        }
        match e.unpacked {
            Some(n) => unpack(&b, n as usize),
            None => Some(b),
        }
    }
}

/// DxLib's LZ: u32 unpacked size, u32 packed size, a control byte, then bytes; the control
/// byte starts a copy (count, offset back) unless it is doubled.
fn unpack(b: &[u8], want: usize) -> Option<Vec<u8>> {
    if b.len() < 9 {
        return None;
    }
    let size = (u32_(b, 0) as usize).min(want.max(1) * 4).max(want);
    let end = (u32_(b, 4) as usize).min(b.len());
    let code = b[8];
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut at = 9;
    while at < end {
        let mut c = b[at];
        at += 1;
        if c != code {
            out.push(c);
            continue;
        }
        c = *b.get(at)?;
        at += 1;
        if c == code {
            out.push(c);
            continue;
        }
        if c > code {
            c -= 1;
        }
        let mut count = (c >> 3) as usize;
        if c & 4 != 0 {
            count |= (*b.get(at)? as usize) << 5;
            at += 1;
        }
        count += 4;
        let off = match c & 3 {
            0 => {
                at += 1;
                *b.get(at - 1)? as usize
            }
            1 => {
                at += 2;
                u16_(b.get(at - 2..at)?, 0) as usize
            }
            2 => {
                at += 3;
                u16_(b.get(at - 3..at)?, 0) as usize | (b[at - 1] as usize) << 16
            }
            _ => return None,
        } + 1;
        if off > out.len() {
            return None;
        }
        let from = out.len() - off;
        for i in 0..count {
            let v = out[from + i];
            out.push(v);
        }
    }
    Some(out)
}

/// The archives of the game, by name: "scr", "gra", "gra2", "jpg", "bgm", "cv", "se" (from
/// `md_<name>.med`).
pub struct Archives {
    pub packs: HashMap<String, Pack>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut packs = HashMap::new();
        for path in files.list() {
            let base = path.rsplit('/').next().unwrap_or(&path).to_lowercase();
            let Some(name) = base
                .strip_prefix("md_")
                .and_then(|n| n.strip_suffix(".med"))
            else {
                continue;
            };
            if path.matches('/').count() > 1 || packs.contains_key(name) {
                continue;
            }
            if let Some(p) = Pack::open(files, &path) {
                packs.insert(name.to_string(), p);
            }
        }
        if packs.is_empty() {
            // no listing: the usual names at the top
            for name in ["scr", "gra", "gra2", "jpg", "bgm", "cv", "se"] {
                if let Some(p) = Pack::open(files, &format!("md_{name}.med")) {
                    packs.insert(name.to_string(), p);
                }
            }
        }
        Archives { packs }
    }

    pub fn read(&self, files: &mut dyn Files, pack: &str, name: &str) -> Option<Vec<u8>> {
        self.packs.get(pack)?.read(files, name)
    }

    pub fn has(&self, pack: &str, name: &str) -> bool {
        self.packs.get(pack).is_some_and(|p| p.has(name))
    }
}
