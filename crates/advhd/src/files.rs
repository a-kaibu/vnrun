//! The game's files: the .arc archives in the game folder, and the save folder.
//!
//! ```text
//! arc   = count:u32 index_size:u32 count x entry  data
//! entry = size:u32 offset:u32 name:UTF-16LE NUL       (offset from the start of data)
//! ```

use std::collections::HashMap;

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

#[derive(Debug, Clone)]
struct Entry {
    arc: usize,
    offset: u64,
    size: usize,
}

/// Every file of every archive, by lower-case name. Archives listed later win, as patches do.
#[derive(Default)]
pub struct Archives {
    arcs: Vec<String>,
    files: HashMap<String, Entry>,
}

pub fn parse_index(head: &[u8]) -> Result<Vec<(String, u64, usize)>, String> {
    let u32_at = |i: usize| -> Result<u32, String> {
        head.get(i..i + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| "index is cut short".to_string())
    };
    let n = u32_at(0)? as usize;
    let size = u32_at(4)? as usize;
    let base = 8 + size as u64;
    let mut out = Vec::with_capacity(n.min(1 << 16));
    let mut at = 8;
    for _ in 0..n {
        let (len, off) = (u32_at(at)? as usize, u32_at(at + 4)? as u64);
        at += 8;
        let mut name = Vec::new();
        loop {
            let c = head.get(at..at + 2).ok_or("index is cut short")?;
            at += 2;
            let c = u16::from_le_bytes([c[0], c[1]]);
            if c == 0 {
                break;
            }
            name.push(c);
        }
        out.push((String::from_utf16_lossy(&name), base + off, len));
    }
    Ok(out)
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Archives {
        let mut a = Archives::default();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| n.to_lowercase().ends_with(".arc"))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        for name in names {
            let Some(head) = files.read_range(&name, 0, 8) else {
                continue;
            };
            let size = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
            let Some(index) = files.read_range(&name, 0, 8 + size) else {
                continue;
            };
            match parse_index(&index) {
                Ok(entries) => {
                    let arc = a.arcs.len();
                    a.arcs.push(name);
                    for (n, offset, size) in entries {
                        a.files
                            .insert(n.to_lowercase(), Entry { arc, offset, size });
                    }
                }
                Err(e) => eprintln!("advhd: {name}: {e}"),
            }
        }
        a
    }

    pub fn has(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.files.get(&name.to_lowercase())?;
        files.read_range(&self.arcs[e.arc], e.offset, e.size)
    }

    /// The archive a file is in.
    pub fn arc_of(&self, name: &str) -> Option<&str> {
        self.files
            .get(&name.to_lowercase())
            .map(|e| self.arcs[e.arc].as_str())
    }

    /// Names (lower case) with this extension.
    pub fn names_with(&self, ext: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .files
            .keys()
            .filter(|n| n.ends_with(ext))
            .cloned()
            .collect();
        v.sort();
        v
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
