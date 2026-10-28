//! The game folder: GameData/*.pack, and the key file some archives need.

use nexas::files::Files;

/// key.fkey (next to the archives, the game or in DLL), or the RESKEY resource of the exe.
pub fn key_file(files: &mut dyn Files) -> Option<Vec<u8>> {
    for p in ["GameData/key.fkey", "key.fkey", "DLL/key.fkey"] {
        if let Some(b) = files.read(p) {
            return Some(b);
        }
    }
    let exes: Vec<String> = files
        .list()
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".exe"))
        .collect();
    for exe in exes {
        if let Some(pe) = files.read(&exe)
            && let Some(k) = crate::pe::resource(&pe, "#10", "RESKEY")
        {
            return Some(k);
        }
    }
    None
}

use std::collections::HashMap;

use crate::pack::Pack;

/// A file name as the index keeps it: lower case, `/` between folders.
pub fn norm(path: &str) -> String {
    let p = path.trim().replace('\\', "/").to_lowercase();
    // `..` and `.` in the path
    let mut parts: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

pub struct Game {
    pub files: Box<dyn Files>,
    packs: Vec<Pack>,
    /// file name -> archive and entry
    index: HashMap<String, (usize, usize)>,
}

impl Game {
    /// Opens GameData/data*.pack (later archives win, as patches do).
    pub fn open(mut files: Box<dyn Files>) -> Result<Game, String> {
        let key = key_file(files.as_mut());
        let mut names: Vec<String> = Vec::new();
        for i in 0..64 {
            let n = format!("GameData/data{i}.pack");
            if files.size(&n).is_some() {
                names.push(n);
            }
        }
        // other .pack files in GameData are not listed by the host; data*.pack is the rule
        let mut g = Game {
            files,
            packs: Vec::new(),
            index: HashMap::new(),
        };
        for n in names {
            match Pack::open(g.files.as_mut(), &n, key.clone()) {
                Ok(p) => {
                    let k = g.packs.len();
                    for (i, e) in p.entries.iter().enumerate() {
                        g.index.insert(norm(&e.name), (k, i));
                    }
                    g.packs.push(p);
                }
                Err(e) => eprintln!("qlie: {e}"),
            }
        }
        if g.packs.is_empty() {
            return Err("no GameData/data*.pack archives".into());
        }
        Ok(g)
    }

    pub fn exists(&mut self, path: &str) -> bool {
        let n = norm(path);
        self.index.contains_key(&n) || self.files.size(&format!("GameData/{n}")).is_some()
    }

    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let n = norm(path);
        if let Some(&(k, i)) = self.index.get(&n) {
            let e = self.packs[k].entries[i].clone();
            return self.packs[k].read_entry(self.files.as_mut(), &e);
        }
        self.files.read(&format!("GameData/{n}"))
    }

    /// Every file name in the archives.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.index.keys()
    }
}
