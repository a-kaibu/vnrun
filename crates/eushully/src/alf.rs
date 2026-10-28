//! The game's files: `sys4ini.bin` (or `sys3ini.bin`) indexes the ALF archives. It starts
//! "S4IC" (0x134) or "S4AC" (an append index, 0x114), then a packed size and the LZSS-packed
//! index: the archive count, their 0x100-byte names, the file count, and per file a 0x40-byte
//! name, the archive number, a number, the offset and the size. A file's place in that list is
//! its number in the scripts (`call-script 0x25d`, `set-texture 0x22c ...`); "@" marks a free
//! number. SYS5 (`sys5ini.bin`, 2024) writes the same index in UTF-16: "S5IC" (or "S5AC" for an
//! append index) in UTF-16, the unpacked sizes and packed size at 0x21c (0x214), archive names
//! of 0x200 bytes and per file 0x90 bytes (a 0x80-byte name, then the four numbers). "S5IN"
//! (trials) is the same index unpacked, from 0x21c.

use std::collections::HashMap;

/// LZSS as the engine packs it: a 4 KiB window starting at 0xfee, flag bits low first, 1 a
/// literal byte, 0 a 12-bit position and 4-bit length (+3).
pub fn lzss(src: &[u8], size: usize) -> Vec<u8> {
    let mut win = [0u8; 4096];
    let mut wp = 0xfee;
    let mut out = Vec::with_capacity(size.min(src.len().saturating_mul(9)));
    let mut i = 0;
    while i < src.len() && out.len() < size {
        let flags = src[i];
        i += 1;
        for b in 0..8 {
            if i >= src.len() || out.len() >= size {
                break;
            }
            if flags & (1 << b) != 0 {
                let c = src[i];
                i += 1;
                out.push(c);
                win[wp] = c;
                wp = (wp + 1) & 0xfff;
            } else {
                if i + 1 >= src.len() {
                    i = src.len();
                    break;
                }
                let (lo, hi) = (src[i] as usize, src[i + 1] as usize);
                i += 2;
                let off = lo | ((hi & 0xf0) << 4);
                for k in 0..(hi & 0xf) + 3 {
                    let c = win[(off + k) & 0xfff];
                    out.push(c);
                    win[wp] = c;
                    wp = (wp + 1) & 0xfff;
                }
            }
        }
    }
    out
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    nexas::sjis::decode(&b[..end])
}

fn wstr(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub arc: String,
    pub offset: u64,
    pub size: usize,
}

/// The files of the game by number, and the numbers by upper-case name.
#[derive(Default)]
pub struct Index {
    pub files: Vec<Option<Entry>>,
    pub by_name: HashMap<String, usize>,
}

impl Index {
    /// Adds the files of an index (`sys4ini.bin`, or an append index after it).
    pub fn add(&mut self, ini: &[u8]) -> Result<(), String> {
        // where the packed size is, where the packed index starts, and the name and record sizes
        let (size_at, data_at, v5) = match ini.get(..8) {
            Some(b) if b.starts_with(b"S4AC") => (0x114, 0x118, false),
            Some(b) if b.starts_with(b"S4IC") || b.starts_with(b"S3IC") => (0x134, 0x138, false),
            Some(b"S\x005\0A\0C\0") => (0x21c, 0x220, true),
            Some(b"S\x005\0I\0C\0") => (0x224, 0x228, true),
            Some(b"S\x005\0I\0N\0") => (0, 0x21c, true),
            _ => return Err("sys4ini: not an index".into()),
        };
        let idx = if size_at == 0 {
            ini.get(data_at..).ok_or("sys5ini: short")?.to_vec()
        } else {
            let packed = u32_at(ini, size_at).ok_or("sys4ini: short")? as usize;
            let src = ini.get(data_at..data_at + packed).ok_or("sys4ini: short")?;
            lzss(src, usize::MAX)
        };
        let (arc_len, name_len, rec_len) = if v5 {
            (0x200, 0x80, 0x90)
        } else {
            (0x100, 0x40, 0x50)
        };
        let name = |b: &[u8]| if v5 { wstr(b) } else { cstr(b) };
        let arcs_n = u32_at(&idx, 0).ok_or("index: short")? as usize;
        let mut p = 4;
        let mut arcs = Vec::new();
        for _ in 0..arcs_n {
            arcs.push(name(idx.get(p..p + arc_len).ok_or("index: short")?));
            p += arc_len;
        }
        let n = u32_at(&idx, p).ok_or("index: short")? as usize;
        p += 4;
        for i in 0..n {
            let rec = idx.get(p..p + rec_len).ok_or("index: short")?;
            p += rec_len;
            let name = name(&rec[..name_len]);
            let arc = u32_at(rec, name_len).unwrap_or(0) as usize;
            let entry = if name == "@" || arc >= arcs.len() {
                None
            } else {
                Some(Entry {
                    name: name.clone(),
                    arc: arcs[arc].clone(),
                    offset: u32_at(rec, name_len + 8).unwrap_or(0) as u64,
                    size: u32_at(rec, name_len + 12).unwrap_or(0) as usize,
                })
            };
            if i >= self.files.len() {
                self.files.resize(i + 1, None);
            }
            if let Some(e) = entry {
                self.by_name.insert(name.to_uppercase(), i);
                self.files[i] = Some(e);
            }
        }
        Ok(())
    }

    pub fn get(&self, n: usize) -> Option<&Entry> {
        self.files.get(n)?.as_ref()
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_uppercase()).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lzss_literals_and_copy() {
        // 3 literals "abc", then a copy of 3 from window position 0xfee
        let src = [0b0000_0111, b'a', b'b', b'c', 0xee, 0xf0];
        assert_eq!(lzss(&src, 6), b"abcabc");
    }
}
