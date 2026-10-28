//! The game's files: loose files in the game folder and the YPF archives next to the engine
//! (cg.ypf holds cg/..., and so on), and the save folder.

use std::collections::HashMap;

use crate::{sjis, ypf};

/// Where the engine reads the game and keeps saves; the browser and native builds each have one.
pub trait Files {
    /// The size of a file of the game, if it exists.
    fn size(&mut self, path: &str) -> Option<u64>;
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// The names in a folder of the game ("" is the top).
    fn list(&mut self, dir: &str) -> Vec<String>;
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
    /// Reads the index of every .ypf at the top of the game folder and in pac/, where older
    /// games keep them (pac/data.ypf).
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives { packs: Vec::new(), names: HashMap::new() };
        let mut found = Vec::new();
        for dir in ["", "pac"] {
            let mut names = files.list(dir);
            names.sort();
            let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            found.extend(names.into_iter().map(|n| format!("{prefix}{n}")));
        }
        for name in found {
            if !name.to_lowercase().ends_with(".ypf") {
                continue;
            }
            let Some(head) = files.read_range(&name, 0, 16) else { continue };
            let Some(start) = head.get(12..16).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { continue };
            // (older archives give the index size without the 32-byte header)
            let Some(head) = files.read_range(&name, 0, start as usize + 32) else { continue };
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
        if let Some(&(p, i)) = self.entry(name) {
            let (pack, index) = &self.packs[p];
            let e = &index.entries[i];
            let stored = files.read_range(pack, e.offset as u64, e.packed_size as usize)?;
            return e.unpack(&stored).map_err(|err| eprintln!("{err}")).ok();
        }
        files.read(&name.replace('\\', "/"))
    }

    pub fn exists(&self, files: &mut dyn Files, name: &str) -> bool {
        self.entry(name).is_some() || files.size(&name.replace('\\', "/")).is_some()
    }

    /// The files in a folder ("scenario/"): in the archives and loose, by their names inside it,
    /// sorted.
    pub fn list(&self, files: &mut dyn Files, dir: &str) -> Vec<String> {
        let dir = ypf::fold(dir);
        let dir = dir.trim_end_matches('/');
        let prefix = format!("{dir}/");
        let mut out: Vec<String> = self
            .packs
            .iter()
            .flat_map(|(_, index)| &index.entries)
            .filter(|e| ypf::fold(&e.name).starts_with(&prefix))
            .map(|e| e.name.replace('\\', "/")[prefix.len()..].to_string())
            .filter(|n| !n.contains('/'))
            .collect();
        out.extend(files.list(dir));
        out.sort_by_key(|n| n.to_lowercase());
        out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        out
    }

    fn entry(&self, name: &str) -> Option<&(usize, usize)> {
        self.names.get(&ypf::fold(name)).or_else(|| self.names.get(&ypf::fold(&sjis::archive_name(name)?)))
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
    fn list(&mut self, dir: &str) -> Vec<String> {
        std::fs::read_dir(self.path(dir))
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
