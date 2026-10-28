//! `.aos` archives (after GARbro's `ArcAOS.cs`) and the Huffman coding of `.scr` and `.cmp`
//! entries (after `HuffmanCompression.cs`).
//!
//! Version 2 (2009 on): u32 0, u32 data start, u32 index size, the archive's name, then at
//! 0x111 one 0x28-byte record per file (name 0x20, offset from the data start, size).
//! Version 1 (2006-2007): 0x20-byte records (name 0x10, offset, size) each followed by its
//! data; a record of 16 0xff bytes says how far the next record is, 16 zero bytes end it.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    /// the archive's file name
    pub arc: String,
    pub name: String,
    pub offset: u64,
    pub size: u32,
}

impl Entry {
    /// `.scr` scripts and `.cmp` pictures are Huffman coded.
    pub fn packed(&self) -> bool {
        let n = self.name.to_ascii_lowercase();
        n.ends_with(".scr") || n.ends_with(".cmp")
    }
}

fn u32at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn name_of(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    encoding_rs::SHIFT_JIS.decode(&b[..end]).0.into_owned()
}

/// Reads an archive's index.
pub fn index(files: &mut dyn Files, arc: &str) -> Result<Vec<Entry>, String> {
    let len = files.size(arc).ok_or(format!("{arc}: missing"))?;
    let head = files
        .read_range(arc, 0, 0x111.min(len as usize))
        .ok_or(format!("{arc}: unreadable"))?;
    if head.len() >= 12 && u32at(&head, 0) == Some(0) {
        // version 2
        let base = u32at(&head, 4).unwrap_or(0) as u64;
        let size = u32at(&head, 8).unwrap_or(0) as usize;
        if 0x111 + size as u64 > len || base > len {
            return Err(format!("{arc}: not an AOS archive"));
        }
        let idx = files
            .read_range(arc, 0x111, size)
            .ok_or(format!("{arc}: index unreadable"))?;
        let mut out = Vec::new();
        for r in idx.chunks_exact(0x28) {
            let name = name_of(&r[..0x20]);
            if name.is_empty() {
                break;
            }
            out.push(Entry {
                arc: arc.to_string(),
                name,
                offset: base + u32at(r, 0x20).unwrap_or(0) as u64,
                size: u32at(r, 0x24).unwrap_or(0),
            });
        }
        return Ok(out);
    }
    // version 1: walk the records, reading the file in pieces
    let mut out = Vec::new();
    let mut at = 0u64;
    let mut buf: Vec<u8> = Vec::new();
    let mut buf_at = u64::MAX;
    while at + 0x20 <= len {
        if buf_at == u64::MAX || at < buf_at || at + 0x20 > buf_at + buf.len() as u64 {
            let n = (len - at).min(0x10000) as usize;
            buf = files
                .read_range(arc, at, n)
                .ok_or(format!("{arc}: unreadable"))?;
            buf_at = at;
        }
        let r = &buf[(at - buf_at) as usize..(at - buf_at) as usize + 0x20];
        if r[..16].iter().all(|&c| c == 0xff) {
            at += 0x20 + u32at(r, 0x10).unwrap_or(0) as u64;
            continue;
        }
        let name = name_of(&r[..16]);
        if name.trim().is_empty() {
            break;
        }
        let offset = u32at(r, 0x10).unwrap_or(0) as u64 + at + 0x20;
        let size = u32at(r, 0x14).unwrap_or(0);
        if offset + size as u64 > len {
            return Err(format!("{arc}: not an AOS archive"));
        }
        out.push(Entry {
            arc: arc.to_string(),
            name,
            offset,
            size,
        });
        at += 0x20;
    }
    Ok(out)
}

/// Every `.aos` of the game folder, by lower-case file name.
pub struct Archives {
    by_name: HashMap<String, Entry>,
    /// lower-case stem -> names with that stem (for lookups without an extension)
    by_stem: HashMap<String, Vec<String>>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Result<Archives, String> {
        let mut by_name = HashMap::new();
        let mut by_stem: HashMap<String, Vec<String>> = HashMap::new();
        let mut any = false;
        for name in files.list() {
            if !name.to_ascii_lowercase().ends_with(".aos") {
                continue;
            }
            any = true;
            for e in index(files, &name)? {
                let key = e.name.to_lowercase();
                let stem = key
                    .rsplit_once('.')
                    .map_or(key.as_str(), |s| s.0)
                    .to_string();
                by_stem.entry(stem).or_default().push(key.clone());
                by_name.entry(key).or_insert(e);
            }
        }
        if !any {
            return Err("no .aos archives".into());
        }
        Ok(Archives { by_name, by_stem })
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(&name.to_lowercase())
    }

    /// A file by its name without extension, trying the extensions in order.
    pub fn find(&self, stem: &str, exts: &[&str]) -> Option<&Entry> {
        let names = self.by_stem.get(&stem.to_lowercase())?;
        for x in exts {
            if let Some(n) = names.iter().find(|n| n.ends_with(x)) {
                return self.by_name.get(n);
            }
        }
        None
    }

    pub fn names(&self) -> impl Iterator<Item = &Entry> {
        self.by_name.values()
    }

    /// A file's bytes, unpacked.
    pub fn read(&self, files: &mut dyn Files, e: &Entry) -> Option<Vec<u8>> {
        let b = files.read_range(&e.arc, e.offset, e.size as usize)?;
        if e.packed() && b.len() >= 4 {
            Some(huffman(&b))
        } else {
            Some(b)
        }
    }
}

/// u32 size, then a bit-coded tree (1: a node with two subtrees, 0: a leaf with an 8-bit
/// byte) and the bits of each byte, most significant bit first.
pub fn huffman(b: &[u8]) -> Vec<u8> {
    let n = u32at(b, 0).unwrap_or(0) as usize;
    let bits = &b[4..];
    let mut pos = 0usize;
    let total = bits.len() * 8;
    let bit = |pos: &mut usize| -> Option<u32> {
        if *pos >= total {
            return None;
        }
        let v = (bits[*pos >> 3] >> (7 - (*pos & 7))) & 1;
        *pos += 1;
        Some(v as u32)
    };
    let mut lhs = [0u16; 512];
    let mut rhs = [0u16; 512];
    let mut token = 256u16;
    // the tree, without recursion: a stack of nodes waiting for their left or right child
    let mut root: Option<u16> = None;
    let mut stack: Vec<(u16, bool)> = Vec::new();
    loop {
        let Some(b) = bit(&mut pos) else {
            return Vec::new();
        };
        let v = if b == 1 {
            if token >= 512 {
                return Vec::new();
            }
            let v = token;
            token += 1;
            v
        } else {
            let mut x = 0u32;
            for _ in 0..8 {
                x = x * 2 + bit(&mut pos).unwrap_or(0);
            }
            x as u16
        };
        match stack.last_mut() {
            None => root = Some(v),
            Some((p, right)) => {
                if *right {
                    rhs[*p as usize] = v;
                    stack.pop();
                } else {
                    lhs[*p as usize] = v;
                    *right = true;
                }
            }
        }
        if b == 1 {
            stack.push((v, false));
        }
        if stack.is_empty() {
            break;
        }
    }
    let root = root.unwrap_or(0);
    let mut out = Vec::with_capacity(n);
    'outer: while out.len() < n {
        let mut s = root;
        while s >= 256 {
            let Some(b) = bit(&mut pos) else {
                break 'outer;
            };
            s = if b == 1 {
                rhs[s as usize]
            } else {
                lhs[s as usize]
            };
        }
        out.push(s as u8);
    }
    out
}
