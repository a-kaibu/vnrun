//! The game's files: every NOA archive of the game folder (and the archives inside them),
//! opened with the passwords the exe lists, looked up by name without case.

use std::collections::HashMap;

use nexas::files::Files;

use crate::noa::{Entry, Noa};

/// Every file under `root`, '/' separated (native builds).
pub fn walk_dir(root: &std::path::Path) -> Vec<String> {
    fn go(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let Ok(name) = e.file_name().into_string() else {
                continue;
            };
            let p = format!("{prefix}{name}");
            if e.path().is_dir() {
                go(&e.path(), &format!("{p}/"), out);
            } else {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    go(root, "", &mut out);
    out.sort();
    out
}

pub struct Fs {
    pub host: Box<dyn Files>,
    pub archives: Vec<Noa>,
    /// lower-case name (with and without the archive's own path) → (archive, entry)
    names: HashMap<String, (usize, usize)>,
    /// the folder that holds the archives ("Data/" or "")
    pub data_dir: String,
}

impl Fs {
    /// `paths`: every file of the game folder, '/' separated.
    pub fn open(mut host: Box<dyn Files>, paths: &[String]) -> Result<Fs, String> {
        let mut keys = Vec::new();
        for p in paths.iter().filter(|p| p.to_lowercase().ends_with(".exe")) {
            if let Some(exe) = host.read(p) {
                keys.extend(crate::noa::exe_keys(&exe));
            }
        }
        let mut fs = Fs {
            host,
            archives: Vec::new(),
            names: HashMap::new(),
            data_dir: String::new(),
        };
        let mut noas: Vec<&String> = paths
            .iter()
            .filter(|p| p.to_lowercase().ends_with(".noa"))
            .collect();
        noas.sort();
        for p in noas {
            if let Some(i) = p.rfind('/') {
                fs.data_dir = p[..=i].to_string();
            }
            let mut noa = Noa::open(fs.host.as_mut(), p, 0)?;
            let base = p.rsplit('/').next().unwrap_or(p);
            noa.password = keys
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(base))
                .map(|(_, k)| k.clone());
            if noa.encrypted() && noa.password.is_none() {
                eprintln!("{p}: encrypted, but the exe has no password for it");
            }
            fs.mount(noa, &keys);
        }
        Ok(fs)
    }

    fn mount(&mut self, noa: Noa, keys: &[(String, String)]) {
        let ai = self.archives.len();
        let mut nested = Vec::new();
        // "title.noa/system.inf" names a file of system/title.noa
        let arc = Fs::key(noa.file.rsplit('/').next().unwrap_or(&noa.file));
        for (ei, e) in noa.entries.iter().enumerate() {
            let lower = e.name.to_lowercase();
            if lower.ends_with(".noa") && e.enc == 0 {
                nested.push(e.clone());
            }
            self.names.insert(format!("{arc}/{lower}"), (ai, ei));
            self.names.entry(lower).or_insert((ai, ei));
        }
        let file = noa.file.clone();
        self.archives.push(noa);
        for e in nested {
            match Noa::open(self.host.as_mut(), &file, e.offset + 16) {
                Ok(mut inner) => {
                    inner.password = keys
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(&e.name))
                        .map(|(_, k)| k.clone());
                    let prefix = e.name.to_lowercase();
                    let ai = self.archives.len();
                    for (ei, ie) in inner.entries.iter().enumerate() {
                        let lower = ie.name.to_lowercase();
                        self.names.insert(format!("{prefix}/{lower}"), (ai, ei));
                        self.names.entry(lower).or_insert((ai, ei));
                    }
                    self.archives.push(inner);
                }
                Err(err) => eprintln!("{}: {err}", e.name),
            }
        }
    }

    pub fn entries(&self) -> Vec<(String, Entry)> {
        let mut v: Vec<(String, Entry)> = Vec::new();
        for a in &self.archives {
            for e in &a.entries {
                v.push((a.file.clone(), e.clone()));
            }
        }
        v
    }

    fn key(name: &str) -> String {
        let n = name.replace('\\', "/").to_lowercase();
        n.trim_start_matches("./").to_string()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn find(&self, name: &str) -> Option<(usize, usize)> {
        let k = Fs::key(name);
        if let Some(v) = self.names.get(&k) {
            return Some(*v);
        }
        // "Data/x.noa/a.eri" or "x.noa/a.eri"
        let base = k.rsplit('/').next().unwrap_or(&k);
        let mut parts = k.rsplitn(3, '/');
        let _ = parts.next();
        if let Some(arc) = parts.next() {
            if let Some(v) = self.names.get(&format!("{arc}/{base}")) {
                return Some(*v);
            }
        }
        self.names.get(base).copied()
    }

    /// A file from an archive, or else from the game folder.
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        if let Some((a, e)) = self.find(name) {
            let noa = &self.archives[a];
            let entry = noa.entries[e].clone();
            return noa
                .read(self.host.as_mut(), &entry)
                .map_err(|err| eprintln!("{err}"))
                .ok();
        }
        let n = name.replace('\\', "/");
        self.host
            .read(&n)
            .or_else(|| self.host.read(&format!("{}{n}", self.data_dir)))
    }
}
