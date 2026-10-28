//! The `.arc` archives of Silky's newer engine (AI6WIN generation): the index size (u32 LE),
//! then for each file a name length byte, the name (each byte lowered by its distance from the end), and the
//! packed size, unpacked size and offset as big-endian u32s. Packed files are LZSS.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Debug, Clone)]
pub struct Entry {
    pub arc: String,
    pub name: String,
    pub packed: u32,
    pub size: u32,
    pub offset: u32,
}

/// Every archive of the game folder, looked up by file name (case-insensitive, any archive).
#[derive(Default)]
pub struct Archives {
    files: HashMap<String, Entry>,
}

pub fn parse_index(arc: &str, b: &[u8], total: u64) -> Result<Vec<Entry>, String> {
    let bad = || format!("{arc}: not a Silky's archive");
    let end = 4 + u32::from_le_bytes(b.get(..4).ok_or_else(bad)?.try_into().unwrap()) as usize;
    if end as u64 > total {
        return Err(bad());
    }
    let mut p = 4;
    let mut out = Vec::new();
    while p < end {
        let n = *b.get(p).ok_or_else(bad)? as usize;
        let raw = b.get(p + 1..p + 1 + n).ok_or_else(bad)?;
        let name: Vec<u8> = raw
            .iter()
            .enumerate()
            .map(|(i, &c)| c.wrapping_add((n - i) as u8))
            .collect();
        p += 1 + n;
        let f = b.get(p..p + 12).ok_or_else(bad)?;
        let be = |i: usize| u32::from_be_bytes(f[i..i + 4].try_into().unwrap());
        p += 12;
        let e = Entry {
            arc: arc.to_string(),
            name: nexas::sjis::decode(&name),
            packed: be(0),
            size: be(4),
            offset: be(8),
        };
        if e.offset as u64 + e.packed as u64 > total || e.name.chars().any(|c| c < ' ') {
            return Err(bad());
        }
        out.push(e);
    }
    Ok(out)
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives::default();
        for name in files.list() {
            if !name.to_lowercase().ends_with(".arc") {
                continue;
            }
            let Some(total) = files.size(&name) else {
                continue;
            };
            // The index: grow the read until it holds every entry.
            let mut want = 64 * 1024usize;
            loop {
                let n = want.min(total as usize);
                let Some(b) = files.read_range(&name, 0, n) else {
                    break;
                };
                match parse_index(&name, &b, total) {
                    Ok(v) => {
                        for e in v {
                            a.files.entry(e.name.to_lowercase()).or_insert(e);
                        }
                        break;
                    }
                    Err(e) if n as u64 >= total || want > 64 << 20 => {
                        eprintln!("{e}");
                        break;
                    }
                    Err(_) => want *= 4,
                }
            }
        }
        a
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.files.get(&name.to_lowercase())
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.files.values()
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entry(name)?;
        let raw = files.read_range(&e.arc, e.offset as u64, e.packed as usize)?;
        Some(if e.packed != e.size {
            lzss(&raw, e.size as usize)
        } else {
            raw
        })
    }
}

/// The usual LZSS: 4 KiB window from 0xfee, flag bits low first (1 = literal).
pub fn lzss(d: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut win = [0u8; 4096];
    let mut wp = 0xfee;
    let mut i = 0;
    while out.len() < size && i < d.len() {
        let flags = d[i];
        i += 1;
        for b in 0..8 {
            if out.len() >= size || i >= d.len() {
                break;
            }
            if flags >> b & 1 == 1 {
                let c = d[i];
                i += 1;
                out.push(c);
                win[wp] = c;
                wp = (wp + 1) & 0xfff;
            } else {
                if i + 1 >= d.len() {
                    i = d.len();
                    break;
                }
                let (lo, hi) = (d[i] as usize, d[i + 1] as usize);
                i += 2;
                let off = lo | (hi & 0xf0) << 4;
                for k in 0..(hi & 0xf) + 3 {
                    let c = win[(off + k) & 0xfff];
                    out.push(c);
                    win[wp] = c;
                    wp = (wp + 1) & 0xfff;
                }
            }
        }
    }
    out.truncate(size);
    out
}
