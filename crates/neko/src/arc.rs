//! NEKOPACK4 archives (`bg.pak`, `scr.pak`, ...), after GARbro's NekoSDK/ArcPAK.cs (MIT).
//!
//! "NEKOPACK4A": u32 index size at 10, the index from 14; "NEKOPACK4S": the index from 10. Each
//! entry is u32 name length, the name (Shift_JIS, NUL ended), then u32 offset and u32 size, both
//! XORed with the sum of the name's bytes (as signed bytes). An entry is zlib data whose first 4
//! bytes are XORed with a key that starts at size / 8 + 0x22 and moves 3 bits left a byte, and
//! whose last 4 bytes are the unpacked size.
//!
//! The scripts name files as `pack\name` (`bg\bg_000.bmp` is `bg_000.bmp` in bg.pak); a name
//! inside a pack may hold further backslashes (`obj\small\sc\sc_000.bmp`).

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
}

pub struct Pack {
    /// the file in the game folder
    pub path: String,
    /// by lower-case name
    pub entries: HashMap<String, Entry>,
    /// the names in index order
    pub names: Vec<String>,
}

/// The packs of the game, by lower-case name without ".pak".
pub struct Archives {
    pub packs: HashMap<String, Pack>,
}

impl Pack {
    pub fn open(files: &mut dyn Files, path: &str) -> Option<Pack> {
        let len = files.size(path)?;
        let head = files.read_range(path, 0, 14.min(len as usize))?;
        if head.len() < 14 || &head[..9] != b"NEKOPACK4" {
            return None;
        }
        let n = u32::from_le_bytes(head[10..14].try_into().ok()?) as u64;
        let (start, end) = match head[9] {
            b'A' => (14u64, 14 + n),
            b'S' => (10u64, 10 + n + 12),
            _ => return None,
        };
        let index = files.read_range(path, start, (end.min(len) - start) as usize)?;
        let mut at = 0usize;
        let (mut entries, mut names) = (HashMap::new(), Vec::new());
        while at + 4 <= index.len() {
            let n = u32::from_le_bytes(index[at..at + 4].try_into().ok()?) as usize;
            at += 4;
            if n == 0 || n > 0x100 || at + n + 8 > index.len() {
                break;
            }
            let raw = &index[at..at + n];
            at += n;
            let key = raw.iter().map(|&b| b as i8 as i32).sum::<i32>() as u32;
            let le = |i: usize| u32::from_le_bytes(index[i..i + 4].try_into().unwrap());
            let (offset, size) = (le(at) ^ key, le(at + 4) ^ key);
            at += 8;
            if offset as u64 + size as u64 > len {
                return None;
            }
            let end = raw.iter().position(|&b| b == 0).unwrap_or(n);
            let name = encoding_rs::SHIFT_JIS.decode(&raw[..end]).0.into_owned();
            names.push(name.clone());
            entries.insert(
                name.to_lowercase(),
                Entry {
                    offset: offset as u64,
                    size,
                },
            );
        }
        (!entries.is_empty()).then(|| Pack {
            path: path.to_string(),
            entries,
            names,
        })
    }

    /// An entry's unpacked bytes.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&name.to_lowercase())?;
        let mut b = files.read_range(&self.path, e.offset, e.size as usize)?;
        let mut key = e.size / 8 + 0x22;
        for x in b.iter_mut().take(4) {
            *x ^= key as u8;
            key <<= 3;
        }
        if b.len() < 8 {
            return None;
        }
        let size = u32::from_le_bytes(b[b.len() - 4..].try_into().ok()?) as usize;
        let n = b.len() - 4;
        let out = miniz_oxide::inflate::decompress_to_vec_zlib(&b[..n])
            .or_else(|e| {
                if e.output.is_empty() {
                    Err(())
                } else {
                    Ok(e.output)
                }
            })
            .ok()?;
        (out.len() == size || size == 0 || out.len() > size / 2).then_some(out)
    }
}

impl Archives {
    /// The .pak files at the top of the game folder or in its `gamedata` folder.
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut packs = HashMap::new();
        let top = files.list();
        // A folder listing names only the top (on disk); the browser lists every path.
        let in_gamedata = |n: &str| {
            n.rsplit_once('/')
                .is_some_and(|(d, _)| d.eq_ignore_ascii_case("gamedata"))
        };
        let mut paths: Vec<String> = top
            .iter()
            .filter(|n| n.to_lowercase().ends_with(".pak") && (!n.contains('/') || in_gamedata(n)))
            .cloned()
            .collect();
        if paths.is_empty()
            && let Some(dir) = top.iter().find(|n| n.eq_ignore_ascii_case("gamedata"))
        {
            for p in [
                "ac",
                "anime",
                "bg",
                "bgm",
                "cfg",
                "cg",
                "effect",
                "flow",
                "gt",
                "obj",
                "parts",
                "sc",
                "scr",
                "se",
                "sys",
                "voice",
                "taiken_parts",
                "ev",
                "fg",
                "movie",
            ] {
                paths.push(format!("{dir}/{p}.pak"));
            }
        }
        for p in paths {
            let stem = p
                .rsplit('/')
                .next()
                .unwrap_or(&p)
                .trim_end_matches(".pak")
                .trim_end_matches(".PAK")
                .to_lowercase();
            if let Some(pack) = Pack::open(files, &p) {
                packs.insert(stem, pack);
            }
        }
        Archives { packs }
    }

    /// A file by its script name (`pack\name`; a name without a pack is looked for in all).
    pub fn read(&self, files: &mut dyn Files, path: &str) -> Option<Vec<u8>> {
        let path = path.replace('/', "\\");
        if let Some((pack, name)) = path.split_once('\\')
            && let Some(p) = self.packs.get(&pack.to_lowercase())
            && let Some(b) = p.read(files, name)
        {
            return Some(b);
        }
        self.packs.values().find_map(|p| p.read(files, &path))
    }

    pub fn has(&self, path: &str) -> bool {
        let path = path.replace('/', "\\").to_lowercase();
        match path.split_once('\\') {
            Some((pack, name)) => self
                .packs
                .get(pack)
                .is_some_and(|p| p.entries.contains_key(name)),
            None => false,
        }
    }
}
