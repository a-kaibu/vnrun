//! The packs: `dwq/<xx><n>.gpk` holds pictures, indexed by `<xx><n>.gtb` (the count, the name
//! offsets, the data offsets, then the names); `cdvaw/<xx>.vpk` holds voices, indexed by
//! `<xx>.vtb` (12 bytes each: an 8-byte name and the offset; a last entry gives the end).

use std::collections::HashMap;

/// The entries of a pack: lower-cased name -> (offset, size).
#[derive(Default, Debug)]
pub struct Index {
    pub entries: HashMap<String, (u64, usize)>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl Index {
    /// A GPK index from its .gtb and the size of the .gpk.
    pub fn gtb(t: &[u8], pack_size: u64) -> Option<Index> {
        let n = u32_at(t, 0)? as usize;
        if n == 0 || n > 1_000_000 || 4 + n * 8 > t.len() {
            return None;
        }
        let names = 4 + n * 8;
        let mut list = Vec::with_capacity(n);
        for i in 0..n {
            let no = u32_at(t, 4 + i * 4)? as usize;
            let off = u32_at(t, 4 + n * 4 + i * 4)? as u64;
            let s = names + no;
            let e = s + t.get(s..)?.iter().position(|&c| c == 0)?;
            list.push((nexas::sjis::decode(&t[s..e]).to_lowercase(), off));
        }
        let mut entries = HashMap::new();
        for i in 0..n {
            let end = list.get(i + 1).map_or(pack_size, |e| e.1);
            let (name, off) = &list[i];
            entries.insert(name.clone(), (*off, end.saturating_sub(*off) as usize));
        }
        Some(Index { entries })
    }

    /// A VPK index from its .vtb and the size of the .vpk.
    pub fn vtb(t: &[u8], pack_size: u64) -> Option<Index> {
        let n = (t.len() / 12).checked_sub(1)?;
        let mut entries = HashMap::new();
        for i in 0..n {
            let raw = &t[i * 12..i * 12 + 8];
            let end = raw.iter().position(|&c| c == 0).unwrap_or(8);
            let name = String::from_utf8_lossy(&raw[..end]).to_lowercase();
            let off = u32_at(t, i * 12 + 8)? as u64;
            let next = u32_at(t, i * 12 + 20).map_or(pack_size, |v| v as u64);
            entries.insert(name, (off, next.saturating_sub(off) as usize));
        }
        Some(Index { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtb_entries() {
        // two entries "ab" at 0 and "c" at 10, in a pack of 25 bytes
        let mut t = Vec::new();
        t.extend(2u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend(3u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend(10u32.to_le_bytes());
        t.extend(b"AB\0c\0");
        let i = Index::gtb(&t, 25).unwrap();
        assert_eq!(i.entries["ab"], (0, 10));
        assert_eq!(i.entries["c"], (10, 15));
    }

    #[test]
    fn vtb_entries() {
        let mut t = Vec::new();
        t.extend(b"ff000001");
        t.extend(0u32.to_le_bytes());
        t.extend(b"ff000002");
        t.extend(7u32.to_le_bytes());
        t.extend([0u8; 8]);
        t.extend(20u32.to_le_bytes());
        let i = Index::vtb(&t, 20).unwrap();
        assert_eq!(i.entries["ff000001"], (0, 7));
        assert_eq!(i.entries["ff000002"], (7, 13));
    }
}
