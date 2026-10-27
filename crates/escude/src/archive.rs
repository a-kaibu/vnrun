//! Escu:de archives ("ESC-ARC1" / "ESC-ARC2", the .bin files beside the exe).
//!
//! ```text
//! "ESC-ARC" version:u8 seed:u32 count:u32 (XOR key)
//! v1: count x { name[0x80] offset:u32 size:u32 }         XOR the key stream, 4 bytes at a time
//! v2: names_size:u32 (XOR key)
//!     count x { name_at:u32 offset:u32 size:u32 }      XOR the key stream
//!     names[names_size]                                 plain, NUL-terminated
//! ```
//!
//! The key stream (from GARbro's Escude/ArcBIN.cs): seed ^= 0x65AC9365; seed ^= ((seed >> 1 ^
//! seed) >> 3) ^ ((seed << 1 ^ seed) << 3). Files starting with "acp\0" and a big-endian size are
//! LZW packed (`unpack`). A file is named by its archive's name as a folder and its name in it.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Debug, Clone)]
pub struct Entry {
    /// the archive file it is in
    pub file: String,
    pub offset: u64,
    pub size: u64,
}

#[derive(Default)]
pub struct Archives {
    pub entries: HashMap<String, Entry>,
    /// names as they are in the archives, in order
    pub names: Vec<String>,
}

/// How names are looked up: lower case, `\` as `/`.
pub fn key(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

struct Keys(u32);

impl Keys {
    fn next(&mut self) -> u32 {
        let mut s = self.0 ^ 0x65AC_9365;
        s ^= (((s >> 1) ^ s) >> 3) ^ (((s << 1) ^ s) << 3);
        self.0 = s;
        s
    }

    fn decrypt(&mut self, b: &mut [u8]) {
        for c in b.chunks_exact_mut(4) {
            let v = u32::from_le_bytes(c.try_into().unwrap()) ^ self.next();
            c.copy_from_slice(&v.to_le_bytes());
        }
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Archives {
    /// Reads every ESC-ARC archive at the top of the game folder.
    pub fn open(files: &mut dyn Files) -> Result<Archives, String> {
        let mut out = Archives::default();
        let mut names = files.list();
        names.sort();
        for n in names {
            if !n.to_lowercase().ends_with(".bin") {
                continue;
            }
            let Some(head) = files.read_range(&n, 0, 20) else {
                continue;
            };
            if !head.starts_with(b"ESC-ARC") {
                continue;
            }
            out.add(files, &n, &head)?;
        }
        if out.names.is_empty() {
            return Err("no Escu:de archive (ESC-ARC .bin files)".into());
        }
        Ok(out)
    }

    fn add(&mut self, files: &mut dyn Files, file: &str, head: &[u8]) -> Result<(), String> {
        let version = head[7];
        let mut keys = Keys(u32_at(head, 8).unwrap());
        let count = (u32_at(head, 12).unwrap() ^ keys.next()) as usize;
        if count > 1_000_000 {
            return Err(format!("{file}: too many files"));
        }
        let size = files.size(file).unwrap_or(0);
        let read = |files: &mut dyn Files, at: u64, len: usize| {
            files
                .read_range(file, at, len)
                .filter(|b| b.len() == len)
                .ok_or(format!("{file}: cannot read the index"))
        };
        let mut list = Vec::with_capacity(count);
        match version {
            b'1' => {
                let mut index = read(files, 0x10, count * 0x88)?;
                keys.decrypt(&mut index);
                for e in index.chunks_exact(0x88) {
                    let end = e[..0x80].iter().position(|&c| c == 0).unwrap_or(0x80);
                    let name = nexas::sjis::decode(&e[..end]);
                    list.push((name, u32_at(e, 0x80).unwrap(), u32_at(e, 0x84).unwrap()));
                }
            }
            b'2' => {
                let names_size = (u32_at(head, 16).unwrap() ^ keys.next()) as usize;
                let mut index = read(files, 0x14, count * 12)?;
                let names = read(files, 0x14 + count as u64 * 12, names_size)?;
                keys.decrypt(&mut index);
                for e in index.chunks_exact(12) {
                    let at = u32_at(e, 0).unwrap() as usize;
                    let tail = names.get(at..).ok_or(format!("{file}: bad name offset"))?;
                    let end = tail.iter().position(|&c| c == 0).unwrap_or(tail.len());
                    list.push((
                        nexas::sjis::decode(&tail[..end]),
                        u32_at(e, 4).unwrap(),
                        u32_at(e, 8).unwrap(),
                    ));
                }
            }
            v => return Err(format!("{file}: ESC-ARC{} not known", v as char)),
        }
        // Files are named by the archive's name as a folder: gfx.bin's sys\logo.bmp is
        // gfx\sys\logo.bmp.
        let stem = file.rsplit_once('.').map_or(file, |p| p.0).to_lowercase();
        for (name, offset, len) in list {
            if offset as u64 + len as u64 > size {
                return Err(format!("{file}: {name} lies past the end"));
            }
            let name = format!("{stem}\\{name}");
            self.entries.insert(
                key(&name),
                Entry {
                    file: file.to_string(),
                    offset: offset as u64,
                    size: len as u64,
                },
            );
            self.names.push(name);
        }
        Ok(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&key(name))
    }

    /// A file's contents, unpacked.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&key(name))?;
        let b = files.read_range(&e.file, e.offset, e.size as usize)?;
        if b.len() > 8 && b.starts_with(b"acp\0") {
            let n = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
            return unpack(&b[8..], n);
        }
        Some(b)
    }
}

/// The LZW of "acp" files (GARbro's FVP LzwDecoder): codes from 9 bits wide, read high bit
/// first; 0x100 ends, 0x101 widens, 0x102 starts over; a code from 0x103 copies the string
/// that code made plus one byte.
pub fn unpack(b: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; size];
    let mut dict: Vec<usize> = Vec::with_capacity(0x8900);
    let (mut width, mut dst) = (9u32, 0usize);
    let (mut bits, mut have, mut at) = (0u64, 0u32, 0usize);
    while dst < size {
        while have < width {
            bits = (bits << 8) | *b.get(at)? as u64;
            at += 1;
            have += 8;
        }
        let code = ((bits >> (have - width)) & ((1 << width) - 1)) as usize;
        have -= width;
        match code {
            0x100 => break,
            0x101 => {
                width += 1;
                if width > 24 {
                    return None;
                }
            }
            0x102 => {
                width = 9;
                dict.clear();
            }
            _ => {
                if dict.len() >= 0x8900 {
                    return None;
                }
                dict.push(dst);
                if code < 0x100 {
                    out[dst] = code as u8;
                    dst += 1;
                } else {
                    let t = code - 0x103;
                    if t + 1 >= dict.len() {
                        return None;
                    }
                    let src = dict[t];
                    let count = (size - dst).min(dict[t + 1] - src + 1);
                    for k in 0..count {
                        out[dst + k] = out[src + k];
                    }
                    dst += count;
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codes written high bit first, 9 bits each.
    fn pack(codes: &[u32]) -> Vec<u8> {
        let (mut out, mut bits, mut have) = (Vec::new(), 0u64, 0);
        for &c in codes {
            bits = (bits << 9) | c as u64;
            have += 9;
            while have >= 8 {
                out.push((bits >> (have - 8)) as u8);
                have -= 8;
            }
        }
        if have > 0 {
            out.push((bits << (8 - have)) as u8);
        }
        out
    }

    #[test]
    fn unpacks_lzw() {
        // "ab", then code 0x103 (the string at entry 0, "a", plus the next byte "b") = "ab"
        let b = pack(&[b'a' as u32, b'b' as u32, 0x103, 0x100]);
        assert_eq!(unpack(&b, 4).unwrap(), b"abab");
    }
}
