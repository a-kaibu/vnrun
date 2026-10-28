//! GAMEDAT PAC archives (`game_sys.dat`, `graph.dat`, `bgm.dat`, ...): "GAMEDAT PAC2" (32-byte
//! names; "PACK" 16-byte ones), the count at 12, the names, then (offset, size) pairs counted
//! from the end of that index.

use std::collections::HashMap;

use nexas::files::Files;

pub struct Pac {
    dats: Vec<String>,
    /// lower-case name: archive, offset, size
    entries: HashMap<String, (usize, u64, usize)>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

impl Pac {
    /// Every top-level `.dat` of the game folder that is a GAMEDAT PAC.
    pub fn open(files: &mut dyn Files) -> Result<Pac, String> {
        let mut pac = Pac {
            dats: Vec::new(),
            entries: HashMap::new(),
        };
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|p| !p.contains('/') && p.to_ascii_lowercase().ends_with(".dat"))
            .collect();
        // game_sys.dat first: its names win
        names.sort_by_key(|n| !n.eq_ignore_ascii_case("game_sys.dat"));
        for dat in names {
            let Some(head) = files.read_range(&dat, 0, 16) else {
                continue;
            };
            if !head.starts_with(b"GAMEDAT PAC") {
                continue;
            }
            let nl = if head[11] == b'K' { 16 } else { 32 };
            let n = u32_at(&head, 12).unwrap_or(0) as usize;
            if n == 0 || n > 1_000_000 {
                continue;
            }
            let idx = files
                .read_range(&dat, 16, n * (nl + 8))
                .ok_or_else(|| format!("{dat}: the index is cut short"))?;
            let base = 16 + (n * (nl + 8)) as u64;
            let a = pac.dats.len();
            for i in 0..n {
                let raw = &idx[i * nl..(i + 1) * nl];
                let end = raw.iter().position(|&c| c == 0).unwrap_or(nl);
                let name = nexas::sjis::decode(&raw[..end]).to_lowercase();
                let off = u32_at(&idx, n * nl + i * 8).unwrap_or(0) as u64;
                let size = u32_at(&idx, n * nl + i * 8 + 4).unwrap_or(0) as usize;
                pac.entries.entry(name).or_insert((a, base + off, size));
            }
            pac.dats.push(dat);
        }
        if pac.dats.is_empty() {
            return Err("no GAMEDAT PAC archive (*.dat) in the game folder".into());
        }
        Ok(pac)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let &(a, off, size) = self.entries.get(&name.to_lowercase())?;
        files.read_range(&self.dats[a], off, size)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }
}

/// An INI file (graphic.def, BGM.ini): lower-case section names, each with its keys (lower case)
/// and values; `;` starts a comment.
pub fn ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if let Some(s) = line.strip_prefix('[') {
            let name = s.trim_end_matches(']').trim().to_lowercase();
            // the first of two sections of one name wins
            cur = match out.entry(name.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => None,
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(HashMap::new());
                    Some(name)
                }
            };
        } else if let (Some(c), Some((k, v))) = (&cur, line.split_once('=')) {
            out.get_mut(c)
                .unwrap()
                .insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    out
}
