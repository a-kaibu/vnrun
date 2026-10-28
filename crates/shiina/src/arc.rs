//! Every .WAR archive of the game folder, by entry name (the scripts name files with a
//! folder, `bg\bg00b.s25`, the archives without).

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;

use crate::warc::{Scheme, Warc};

pub struct Archives {
    arcs: Vec<(String, Warc)>,
    /// lower-case entry name -> (archive, entry)
    by_name: HashMap<String, (usize, usize)>,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Result<Archives, String> {
        let schemes: Vec<Rc<Scheme>> = Scheme::all().into_iter().map(Rc::new).collect();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| n.to_ascii_lowercase().ends_with(".war") && !n.contains('/'))
            .collect();
        names.sort();
        if names.is_empty() {
            return Err("no .WAR archives".into());
        }
        let mut arcs = Vec::new();
        let mut by_name = HashMap::new();
        for n in names {
            let Some(len) = files.size(&n) else {
                continue;
            };
            let mut read = |at: u64, len: usize| files.read_range(&n, at, len).unwrap_or_default();
            let Some(w) = Warc::open(&mut read, len, &schemes) else {
                continue;
            };
            let k = arcs.len();
            for (i, e) in w.entries.iter().enumerate() {
                by_name.entry(e.name.to_lowercase()).or_insert((k, i));
            }
            arcs.push((n, w));
        }
        if arcs.is_empty() {
            return Err("no WARC archive this build has keys for".into());
        }
        Ok(Archives { arcs, by_name })
    }

    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(&name.to_lowercase())
    }

    /// An entry's bytes, decrypted and unpacked.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let &(a, i) = self.by_name.get(&name.to_lowercase())?;
        let (arc, w) = &self.arcs[a];
        let e = &w.entries[i];
        let b = files.read_range(arc, e.offset as u64, e.size as usize)?;
        Some(w.decode(e, b))
    }
}

/// An .OGV sound: a RIFF-like header ("fmt ", "data") before an Ogg stream.
pub fn ogg(b: Vec<u8>) -> Vec<u8> {
    match b.windows(4).take(512).position(|w| w == b"OggS") {
        Some(at) if at > 0 => b[at..].to_vec(),
        _ => b,
    }
}
