//! XFL archives (after GARbro, MIT, by morkt: `Liar/ArcXFL.cs`).
//!
//! ```text
//! "LB" 1 0  dir_size:u32 count:u32
//! count entries of 40 bytes from 12: name ([u8; 32], NUL padded) offset:u32 size:u32
//! the data starts at 12 + dir_size; offsets count from there
//! ```
//!
//! An entry named `*.xfl` is an archive itself (the voices of each chapter, `2.xfl`): its
//! entries are listed as `2.xfl/0001.wav`.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
}

pub struct Xfl {
    /// the file in the game folder
    pub path: String,
    /// by lower-case name (nested ones as `inner.xfl/name`)
    pub entries: HashMap<String, Entry>,
    /// the names in index order
    pub names: Vec<String>,
}

fn u32_(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

impl Xfl {
    pub fn open(files: &mut dyn Files, path: &str) -> Option<Xfl> {
        let len = files.size(path)?;
        let mut x = Xfl {
            path: path.to_string(),
            entries: HashMap::new(),
            names: Vec::new(),
        };
        x.read_dir(files, 0, len, "", 0)?;
        Some(x)
    }

    fn read_dir(
        &mut self,
        files: &mut dyn Files,
        base: u64,
        len: u64,
        prefix: &str,
        depth: u32,
    ) -> Option<()> {
        let head = files.read_range(&self.path, base, 12)?;
        if head.len() < 12 || &head[..4] != b"LB\x01\x00" || depth > 2 {
            return None;
        }
        let (dir_size, count) = (u32_(&head, 4) as u64, u32_(&head, 8) as usize);
        if count == 0 || count > 100_000 || count as u64 * 40 > dir_size || 12 + dir_size > len {
            return None;
        }
        let dir = files.read_range(&self.path, base + 12, count * 40)?;
        let data = base + 12 + dir_size;
        let mut nested = Vec::new();
        for e in dir.chunks_exact(40) {
            let end = e[..32].iter().position(|&c| c == 0).unwrap_or(32);
            let name = encoding_rs::SHIFT_JIS.decode(&e[..end]).0.to_lowercase();
            let (off, size) = (u32_(e, 32) as u64, u32_(e, 36));
            if data + off + size as u64 > base + len {
                return None;
            }
            let full = format!("{prefix}{name}");
            let entry = Entry {
                offset: data + off,
                size,
            };
            if name.ends_with(".xfl") {
                nested.push((full.clone(), entry));
            }
            self.entries.insert(full.clone(), entry);
            self.names.push(full);
        }
        for (name, e) in nested {
            // a broken inner archive leaves its entry as it is
            let _ = self.read_dir(
                files,
                e.offset,
                e.size as u64,
                &format!("{name}/"),
                depth + 1,
            );
        }
        Some(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&name.to_lowercase())?;
        files.read_range(&self.path, e.offset, e.size as usize)
    }
}

/// The archives at the top of the game folder, by lower-case name without `.xfl` ("scr",
/// "grpe", "grpo_bu0", "voice", ...).
pub struct Archives {
    pub packs: HashMap<String, Xfl>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut packs = HashMap::new();
        for path in files.list() {
            let base = path.rsplit('/').next().unwrap_or(&path).to_lowercase();
            let Some(name) = base.strip_suffix(".xfl") else {
                continue;
            };
            if path.matches('/').count() > 1 || packs.contains_key(name) {
                continue;
            }
            if let Some(x) = Xfl::open(files, &path) {
                packs.insert(name.to_string(), x);
            }
        }
        if packs.is_empty() {
            for name in [
                "scr", "grpe", "grpo", "grps", "grpo_bu0", "grpo_bu1", "grpo_ef", "bgm", "voice",
                "wav",
            ] {
                if let Some(x) = Xfl::open(files, &format!("{name}.xfl")) {
                    packs.insert(name.to_string(), x);
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

/// The Ogg Vorbis stream of a WAV whose format is 0x6771 (Ogg in a RIFF wrapper), else the
/// bytes as they are.
pub fn unwrap_ogg(b: Vec<u8>) -> Vec<u8> {
    if b.len() > 44
        && &b[..4] == b"RIFF"
        && b[20..22] == [0x71, 0x67]
        && let Some(at) = b[..b.len().min(512)].windows(4).position(|w| w == b"OggS")
    {
        return b[at..].to_vec();
    }
    b
}
