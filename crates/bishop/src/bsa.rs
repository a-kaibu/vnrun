//! BSArc archives (*.bsa): "BSArc\0\0\0", version (u16 at 8), count (u16 at 0xa), index offset
//! (u32 at 0xc). Version 3 keeps 12-byte entries (name offset, offset, size) and the names after
//! them; older ones 0x28-byte entries (0x20-byte name, offset, size). A name ">dir" enters a
//! folder and "<" leaves it.

use std::collections::HashMap;

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    nexas::sjis::decode(&b[..end])
}

/// The entries of an archive: lower-case path ("system/title/title_base.bsg") -> (offset, size).
#[derive(Default, Debug)]
pub struct Index {
    pub entries: HashMap<String, (u64, usize)>,
}

/// The header: version, count and where the index starts.
pub fn header(b: &[u8]) -> Option<(u16, usize, u64)> {
    if !b.starts_with(b"BSArc\0") {
        return None;
    }
    Some((
        u16_at(b, 8)?,
        u16_at(b, 10)? as usize,
        u32_at(b, 12)? as u64,
    ))
}

impl Index {
    /// `idx` is the archive from the index offset to its end.
    pub fn parse(version: u16, count: usize, idx: &[u8]) -> Index {
        let mut entries = HashMap::new();
        let mut dirs: Vec<String> = Vec::new();
        let mut add = |name: String, off: u32, size: u32, dirs: &mut Vec<String>| {
            if let Some(d) = name.strip_prefix('>') {
                dirs.push(d.to_string());
            } else if name.starts_with('<') {
                dirs.pop();
            } else if !name.is_empty() {
                let mut p = dirs.join("/");
                if !p.is_empty() {
                    p.push('/');
                }
                p.push_str(&name);
                entries.insert(p.to_lowercase(), (off as u64, size as usize));
            }
        };
        if version >= 3 {
            let names = idx.get(count * 12..).unwrap_or(&[]);
            for i in 0..count {
                let (Some(no), Some(off), Some(size)) = (
                    u32_at(idx, i * 12),
                    u32_at(idx, i * 12 + 4),
                    u32_at(idx, i * 12 + 8),
                ) else {
                    break;
                };
                let name = cstr(names.get(no as usize..).unwrap_or(&[]));
                add(name, off, size, &mut dirs);
            }
        } else {
            for i in 0..count {
                let Some(e) = idx.get(i * 0x28..i * 0x28 + 0x28) else {
                    break;
                };
                let name = cstr(&e[..0x20]);
                add(
                    name,
                    u32_at(e, 0x20).unwrap_or(0),
                    u32_at(e, 0x24).unwrap_or(0),
                    &mut dirs,
                );
            }
        }
        Index { entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_folders() {
        let mut idx = Vec::new();
        let names = b">sys\0a.bsg\0<\0b.ogg\0";
        for (no, off, size) in [(0u32, 0u32, 0u32), (5, 16, 3), (11, 0, 0), (13, 19, 4)] {
            idx.extend(no.to_le_bytes());
            idx.extend(off.to_le_bytes());
            idx.extend(size.to_le_bytes());
        }
        idx.extend(names);
        let i = Index::parse(3, 4, &idx);
        assert_eq!(i.entries.get("sys/a.bsg"), Some(&(16, 3)));
        assert_eq!(i.entries.get("b.ogg"), Some(&(19, 4)));
    }
}
