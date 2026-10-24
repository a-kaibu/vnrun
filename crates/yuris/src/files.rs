//! The game's files: loose files in the game folder and the YPF archives next to the engine
//! (cg.ypf holds cg/..., and so on), and the save folder.

use std::collections::HashMap;

use crate::ypf;

/// Where the engine reads the game and keeps saves; the browser and native builds each have one.
pub trait Files {
    /// The size of a file of the game, if it exists.
    fn size(&mut self, path: &str) -> Option<u64>;
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// The files at the top of the game folder.
    fn root(&mut self) -> Vec<String>;
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>>;
    fn write_save(&mut self, name: &str, bytes: &[u8]);
    fn remove_save(&mut self, name: &str);
    /// Shows a message with 1 to 3 buttons; returns which was pressed (1 = the leftmost).
    fn dialog(&mut self, text: &str, caption: &str, buttons: u32) -> i64 {
        let _ = (caption, buttons);
        eprintln!("DIALOG: {text}");
        1
    }

    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let n = self.size(path)?;
        self.read_range(path, 0, n as usize)
    }
}

/// The archives and where each file of them is.
pub struct Archives {
    /// archive file -> its index
    packs: Vec<(String, ypf::Index)>,
    /// folded name -> (archive, entry)
    names: HashMap<String, (usize, usize)>,
}

impl Archives {
    /// Reads the index of every .ypf at the top of the game folder.
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives { packs: Vec::new(), names: HashMap::new() };
        let mut root = files.root();
        root.sort();
        for name in root {
            if !name.to_lowercase().ends_with(".ypf") {
                continue;
            }
            let Some(head) = files.read_range(&name, 0, 16) else { continue };
            let Some(start) = head.get(12..16).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { continue };
            let Some(head) = files.read_range(&name, 0, start as usize) else { continue };
            match ypf::Index::parse(&head) {
                Ok(index) => {
                    let p = a.packs.len();
                    for (i, e) in index.entries.iter().enumerate() {
                        a.names.insert(ypf::fold(&e.name), (p, i));
                    }
                    a.packs.push((name, index));
                }
                Err(e) => eprintln!("{name}: {e}"),
            }
        }
        a
    }

    /// A file of the game by its name in the scripts: from an archive, else loose.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let key = ypf::fold(name);
        if let Some(&(p, i)) = self.names.get(&key) {
            let (pack, index) = &self.packs[p];
            let e = &index.entries[i];
            let stored = files.read_range(pack, e.offset as u64, e.packed_size as usize)?;
            return e.unpack(&stored).map_err(|err| eprintln!("{err}")).ok();
        }
        files.read(&name.replace('\\', "/"))
    }

    pub fn exists(&self, files: &mut dyn Files, name: &str) -> bool {
        self.names.contains_key(&ypf::fold(name)) || files.size(&name.replace('\\', "/")).is_some()
    }

    /// The name of a file given without its extension: the first of `exts` that exists.
    pub fn find(&self, files: &mut dyn Files, name: &str, exts: &[&str]) -> Option<String> {
        if self.exists(files, name) {
            return Some(name.to_string());
        }
        exts.iter().map(|e| format!("{name}.{e}")).find(|n| self.exists(files, n))
    }
}

/// The game folder on disk, for running without a browser.
pub struct Disk {
    pub game: std::path::PathBuf,
    pub saves: std::path::PathBuf,
}

impl Disk {
    fn path(&self, p: &str) -> std::path::PathBuf {
        // names in scripts ignore case; look each part up in its folder
        let mut at = self.game.clone();
        for part in p.split(['/', '\\']).filter(|s| !s.is_empty()) {
            let next = std::fs::read_dir(&at)
                .ok()
                .and_then(|d| {
                    d.flatten().map(|e| e.file_name()).find(|n| n.to_string_lossy().eq_ignore_ascii_case(part))
                })
                .map_or_else(|| at.join(part), |n| at.join(n));
            at = next;
        }
        at
    }
}

impl Files for Disk {
    fn size(&mut self, path: &str) -> Option<u64> {
        std::fs::metadata(self.path(path)).ok().filter(|m| m.is_file()).map(|m| m.len())
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.path(path)).ok()?;
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut out = vec![0; len];
        let n = f.read(&mut out).ok()?;
        out.truncate(n);
        Some(out)
    }
    fn root(&mut self) -> Vec<String> {
        std::fs::read_dir(&self.game)
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.saves.join(name)).ok()
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        let p = self.saves.join(name);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, bytes);
    }
    fn remove_save(&mut self, name: &str) {
        let _ = std::fs::remove_file(self.saves.join(name));
    }
}
