//! The game's files: the PAC archives in the game folder (Script.pac, Visual.pac, ...), and the
//! save folder.

use std::collections::HashMap;

use crate::pac;

/// Where the engine reads the game and keeps saves; the browser and native builds each have one.
pub trait Files {
    /// The size of a file of the game, if it exists.
    fn size(&mut self, path: &str) -> Option<u64>;
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// The names at the top of the game folder.
    fn list(&mut self) -> Vec<String>;
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>>;
    fn write_save(&mut self, name: &str, bytes: &[u8]);

    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let n = self.size(path)?;
        self.read_range(path, 0, n as usize)
    }
}

/// The archives of the game, by name without extension ("Script", "Visual", ...).
pub struct Archives {
    packs: HashMap<String, (String, pac::Index)>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut packs = HashMap::new();
        for name in files.list() {
            let Some(stem) = name
                .strip_suffix(".pac")
                .or_else(|| name.strip_suffix(".PAC"))
            else {
                continue;
            };
            let Some(size) = files.size(&name) else {
                continue;
            };
            let (Some(head), Some(last)) = (
                files.read_range(&name, 0, 12),
                files.read_range(&name, size.saturating_sub(4), 4),
            ) else {
                continue;
            };
            let Some(n) = pac::Index::tail_size(&last) else {
                continue;
            };
            let Some(tail) = files.read_range(&name, size.saturating_sub(n as u64), n) else {
                continue;
            };
            match pac::Index::parse(&head, &tail) {
                Ok(index) => {
                    packs.insert(stem.to_lowercase(), (name.clone(), index));
                }
                Err(e) => eprintln!("{name}: {e}"),
            }
        }
        Archives { packs }
    }

    pub fn has(&self, pack: &str) -> bool {
        self.packs.contains_key(&pack.to_lowercase())
    }

    /// A file of an archive.
    pub fn read(&self, files: &mut dyn Files, pack: &str, name: &str) -> Option<Vec<u8>> {
        let (file, index) = self.packs.get(&pack.to_lowercase())?;
        let e = index.find(name)?;
        let raw = files.read_range(file, e.offset as u64, e.packed_size as usize)?;
        index
            .unpack(e, &raw)
            .map_err(|err| eprintln!("{file}: {err}"))
            .ok()
    }

    /// The names in an archive.
    pub fn names(&self, pack: &str) -> Vec<String> {
        self.packs
            .get(&pack.to_lowercase())
            .map_or(Vec::new(), |(_, i)| {
                i.entries.iter().map(|e| e.name.clone()).collect()
            })
    }
}

/// The game folder on disk (native builds).
pub struct Disk {
    pub root: std::path::PathBuf,
    pub saves: std::path::PathBuf,
}

impl Files for Disk {
    fn size(&mut self, path: &str) -> Option<u64> {
        std::fs::metadata(self.root.join(path))
            .ok()
            .map(|m| m.len())
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.root.join(path)).ok()?;
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0; len];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    }
    fn list(&mut self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)
            .map(|d| {
                d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.saves.join(name)).ok()
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        let _ = std::fs::create_dir_all(&self.saves);
        let _ = std::fs::write(self.saves.join(name), bytes);
    }
}
