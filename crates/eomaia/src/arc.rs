//! The .arc archives (script.arc, image.arc, music.arc, voice.arc, effect.arc, data.arc).
//!
//! A u32 (LE) index length, then entries: a u8 name length, the name, the size twice and the
//! offset from the start of the file (u32 BE each). Each byte of a name is stored as
//! `c - (len + 0x20 - i)`, plus 0x20 for anything but a lowercase letter, so decoding adds the
//! key and takes 0x20 off whatever does not come out as `a..=z`. Names may carry a folder
//! ("image/001.png"); we look files up by the last part, without case.

use std::collections::HashMap;

use nexas::files::Files;

pub const ARCHIVES: [&str; 6] = [
    "script.arc",
    "image.arc",
    "data.arc",
    "music.arc",
    "voice.arc",
    "effect.arc",
];

#[derive(Default)]
pub struct Arcs {
    /// lowercase file name -> (archive, offset, size)
    files: HashMap<String, (usize, u64, usize)>,
}

pub fn decode_name(b: &[u8]) -> String {
    let len = b.len() as i32;
    b.iter()
        .enumerate()
        .map(|(i, &v)| {
            let k = len + 0x20 - i as i32;
            let mut c = (v as i32 + k) & 0xff;
            if !(0x61..=0x7a).contains(&c) {
                c = (c - 0x20) & 0xff;
            }
            c as u8 as char
        })
        .collect()
}

/// The entries of one archive: (name, offset, size).
pub fn index(d: &[u8]) -> Vec<(String, u64, usize)> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < d.len() {
        let l = d[p] as usize;
        let Some(name) = d.get(p + 1..p + 1 + l) else {
            break;
        };
        let at = p + 1 + l;
        let Some(nums) = d.get(at..at + 12) else {
            break;
        };
        let be = |i: usize| u32::from_be_bytes(nums[i..i + 4].try_into().unwrap());
        out.push((decode_name(name), be(8) as u64, be(0) as usize));
        p = at + 12;
    }
    out
}

impl Arcs {
    pub fn open(files: &mut dyn Files) -> Result<Arcs, String> {
        let mut arcs = Arcs::default();
        for (i, a) in ARCHIVES.iter().enumerate() {
            let Some(head) = files.read_range(a, 0, 4) else {
                continue;
            };
            let n = u32::from_le_bytes(head.try_into().unwrap()) as usize;
            let d = files
                .read_range(a, 4, n)
                .ok_or_else(|| format!("{a}: the index is cut short"))?;
            for (name, off, size) in index(&d) {
                let base = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
                arcs.files.insert(base, (i, off, size));
            }
        }
        if arcs.files.is_empty() {
            return Err("no .arc archives (script.arc, image.arc) in the game folder".into());
        }
        Ok(arcs)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_lowercase())
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let &(a, off, size) = self.files.get(&name.to_lowercase())?;
        files.read_range(ARCHIVES[a], off, size)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.files.keys().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(s: &str) -> Vec<u8> {
        let len = s.len() as i32;
        s.bytes()
            .enumerate()
            .map(|(i, c)| {
                let k = len + 0x20 - i as i32;
                let extra = if c.is_ascii_lowercase() { 0 } else { 0x20 };
                ((c as i32 - k + extra) & 0xff) as u8
            })
            .collect()
    }

    #[test]
    fn names_round_trip() {
        for s in [
            "image/001.png",
            "start.script",
            "voice/02_hik101_001.ogg",
            "a_b.json",
        ] {
            assert_eq!(decode_name(&encode(s)), s);
        }
    }
}
