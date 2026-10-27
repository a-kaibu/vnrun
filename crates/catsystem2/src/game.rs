//! The game's files: every .int archive in the game folder, looked up by file name.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;

use crate::hg3::Frame;
use crate::int::Archive;

pub struct Game {
    pub files: Box<dyn Files>,
    archives: Vec<Archive>,
    /// lower-case file name -> archive
    index: HashMap<String, usize>,
    images: HashMap<String, Rc<Vec<Frame>>>,
}

impl Game {
    pub fn open(mut files: Box<dyn Files>) -> Result<Game, String> {
        let key = ["cs2.exe", "CS2.exe", "cs2.EXE"]
            .iter()
            .find_map(|n| files.read(n))
            .or_else(|| {
                // some games rename the exe
                let names = files.list();
                names
                    .iter()
                    .filter(|n| n.to_lowercase().ends_with(".exe"))
                    .find_map(|n| {
                        let pe = files.read(n)?;
                        crate::exe::passphrase(&pe).map(|_| pe)
                    })
            })
            .and_then(|pe| crate::exe::passphrase(&pe))
            .map(|p| crate::int::game_key(&p));
        let mut g = Game {
            files,
            archives: Vec::new(),
            index: HashMap::new(),
            images: HashMap::new(),
        };
        let mut names: Vec<String> = g
            .files
            .list()
            .into_iter()
            .filter(|n| n.to_lowercase().ends_with(".int"))
            .collect();
        names.sort();
        for n in names {
            match Archive::open(g.files.as_mut(), &n, key) {
                Ok(a) => {
                    let i = g.archives.len();
                    for name in &a.names {
                        g.index.entry(name.to_lowercase()).or_insert(i);
                    }
                    g.archives.push(a);
                }
                Err(e) => eprintln!("{n}: {e}"),
            }
        }
        if g.archives.is_empty() {
            return Err("no .int archives in the game folder".into());
        }
        Ok(g)
    }

    /// A file of the archives by name (with extension), or of the game folder.
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        // "config.int/vram.xml" names the archive too
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let i = *self.index.get(&name.to_lowercase())?;
        self.archives[i].read(self.files.as_mut(), name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_lowercase())
    }

    /// The first of `exts` that exists for `stem`.
    pub fn find(&self, stem: &str, exts: &[&str]) -> Option<String> {
        exts.iter()
            .map(|e| format!("{stem}.{e}"))
            .find(|n| self.has(n))
    }

    /// The frames of a picture (name without .hg3).
    pub fn image(&mut self, name: &str) -> Option<Rc<Vec<Frame>>> {
        let key = name.to_lowercase();
        if let Some(f) = self.images.get(&key) {
            return Some(f.clone());
        }
        let data = self.read(&format!("{name}.hg3"))?;
        let frames = match crate::hg3::decode(&data) {
            Ok(f) => Rc::new(f),
            Err(e) => {
                eprintln!("{name}.hg3: {e}");
                return None;
            }
        };
        if self.images.len() > 96 {
            self.images.clear();
        }
        self.images.insert(key, frames.clone());
        Some(frames)
    }

    /// A frame of a picture by id (the first frame for a negative id).
    pub fn frame(&mut self, name: &str, id: i64) -> Option<Frame> {
        let frames = self.image(name)?;
        if id < 0 {
            return frames.iter().min_by_key(|f| f.id).cloned();
        }
        frames.iter().find(|f| f.id as i64 == id).cloned()
    }

    /// A text file of the archives (Shift_JIS).
    pub fn text(&mut self, name: &str) -> Option<String> {
        self.read(name).map(|b| crate::sjis(&b))
    }
}

/// The text of the first `<tag>...</tag>` in `xml` after `from` (0 for the start).
pub fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)? + s;
    Some(&xml[s..e])
}

/// A value at a path of tags: `xml_path(x, &["SCREEN", "width"])`.
pub fn xml_path<'a>(xml: &'a str, path: &[&str]) -> Option<&'a str> {
    let mut cur = xml;
    for t in path {
        cur = xml_tag(cur, t)?;
    }
    Some(cur.trim())
}
