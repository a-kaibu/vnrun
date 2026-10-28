//! The game's files through its packages: every `pkg/*.sx` index (later ones win), the storages
//! they point into, and lookups by bare name as the scenario uses them (`b_00_2.ui`, `se048`).

use std::collections::HashMap;

use crate::pkg::{Entry, Index};
use crate::sx;

/// Where the engine reads the game and keeps saves; the browser and native builds each have one.
pub trait Host {
    /// The size of a file of the game, if it exists.
    fn size(&mut self, path: &str) -> Option<u64>;
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// The names in a folder of the game (`""` for the top).
    fn list(&mut self, dir: &str) -> Vec<String>;
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>>;
    fn write_save(&mut self, name: &str, bytes: &[u8]);

    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let n = self.size(path)?;
        self.read_range(path, 0, n as usize)
    }
}

pub struct Assets {
    pub host: Box<dyn Host>,
    /// path (no leading slash) -> storage file, entry
    files: HashMap<String, (String, Entry)>,
    /// lower-case path -> path
    folded: HashMap<String, String>,
    /// lower-case file name -> paths with it
    names: HashMap<String, Vec<String>>,
    /// The package's name (`dekamikosistry`).
    pub title: String,
}

impl Assets {
    pub fn open(mut host: Box<dyn Host>) -> Result<Assets, String> {
        let mut dirs = vec!["pkg".to_string(), String::new()];
        for n in host.list("") {
            if n.eq_ignore_ascii_case("pkg") {
                dirs[0] = n;
            }
        }
        let mut sx_files = Vec::new();
        for d in &dirs {
            let mut names: Vec<String> = host
                .list(d)
                .into_iter()
                .filter(|n| n.to_ascii_lowercase().ends_with(".sx"))
                .collect();
            names.sort();
            for n in names {
                sx_files.push(if d.is_empty() { n } else { format!("{d}/{n}") });
            }
            if !sx_files.is_empty() {
                break;
            }
        }
        if sx_files.is_empty() {
            return Err("no pkg/*.sx package index".into());
        }
        let mut a = Assets {
            host,
            files: HashMap::new(),
            folded: HashMap::new(),
            names: HashMap::new(),
            title: String::new(),
        };
        for path in sx_files {
            let raw = a
                .host
                .read(&path)
                .ok_or_else(|| format!("{path}: unreadable"))?;
            let ix = Index::parse(&sx::open(&raw).map_err(|e| format!("{path}: {e}"))?)?;
            let file = path.rsplit('/').next().unwrap_or(&path);
            let base = &file[..file.rfind('(').unwrap_or(file.len() - 3)];
            let dir = &path[..path.len() - file.len()];
            if a.title.is_empty() {
                a.title = base.rsplit_once('-').map_or(base, |(t, _)| t).to_string();
            }
            for p in ix.order {
                let e = ix.files[&p];
                let Some(st) = ix.storages.get(e.storage as usize) else {
                    continue;
                };
                let store = format!("{dir}{base}-{}.sxstorage", st.name);
                a.folded.insert(p.to_ascii_lowercase(), p.clone());
                let name = p.rsplit('/').next().unwrap_or(&p).to_ascii_lowercase();
                let list = a.names.entry(name).or_default();
                if !list.contains(&p) {
                    list.push(p.clone());
                }
                a.files.insert(p, (store, e));
            }
        }
        Ok(a)
    }

    /// The stored path for `path` (leading slashes and case ignored).
    pub fn resolve(&self, path: &str) -> Option<&str> {
        let p = path.trim_start_matches('/');
        if let Some((k, _)) = self.files.get_key_value(p) {
            return Some(k);
        }
        self.folded.get(&p.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.resolve(path).is_some()
    }

    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let p = self.resolve(path)?.to_string();
        let (store, e) = self.files.get(&p)?.clone();
        let raw = self
            .host
            .read_range(&store, e.block as u64 * 16, e.size as usize)?;
        match sx::file(&raw, e.flags, e.block) {
            Ok(d) => Some(d),
            Err(err) => {
                eprintln!("{p}: {err}");
                None
            }
        }
    }

    /// A file named `name` (with `ext` added when it has no extension), preferring the folders
    /// in `dirs` in order, then any.
    pub fn find(&self, name: &str, ext: &str, dirs: &[&str]) -> Option<String> {
        let name = name.trim();
        if name.contains('/')
            && let Some(p) = self.resolve(name) {
                return Some(p.to_string());
            }
        let file = name.rsplit('/').next().unwrap_or(name);
        let mut want = file.to_ascii_lowercase();
        if !ext.is_empty() && !want.contains('.') {
            want = format!("{want}.{ext}");
        }
        let list = self.names.get(&want)?;
        for d in dirs {
            let d = d.trim_matches('/').to_ascii_lowercase();
            if let Some(p) = list
                .iter()
                .find(|p| p.to_ascii_lowercase().starts_with(&format!("{d}/")))
            {
                return Some(p.clone());
            }
        }
        list.first().cloned()
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
}
