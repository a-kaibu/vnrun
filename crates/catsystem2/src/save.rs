//! What a save slot holds, in our own format. The story is not snapshotted: a load starts the
//! scene the story last entered (not by `call`) with the variables it had then, and skips
//! through it to the page that was showing; then the variables are set to what they were.

use std::collections::HashMap;

#[derive(Default, Debug, PartialEq)]
pub struct SaveData {
    /// the scene entered last, and the variables when it was entered
    pub entry: String,
    pub entry_flags: Vec<(i64, i64)>,
    pub entry_strs: Vec<(i64, String)>,
    /// the page showing: its scene and the line it starts at
    pub scene: String,
    pub line: u32,
    /// the variables at the page
    pub flags: Vec<(i64, i64)>,
    pub strs: Vec<(i64, String)>,
    /// who spoke and the text, shown in the slot
    pub speaker: String,
    pub text: String,
}

const MAGIC: &[u8; 8] = b"CS2SAVE1";

/// The file of save slot `k` (from 0).
pub fn slot_name(k: usize) -> String {
    format!("cs2save{k:02}.dat")
}

pub fn sorted_flags(m: &HashMap<i64, i64>) -> Vec<(i64, i64)> {
    let mut v: Vec<_> = m.iter().map(|(&k, &v)| (k, v)).collect();
    v.sort();
    v
}

pub fn sorted_strs(m: &HashMap<i64, String>) -> Vec<(i64, String)> {
    let mut v: Vec<_> = m.iter().map(|(&k, v)| (k, v.clone())).collect();
    v.sort();
    v
}

impl SaveData {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(MAGIC.to_vec());
        w.str(&self.entry);
        w.flags(&self.entry_flags);
        w.strs(&self.entry_strs);
        w.str(&self.scene);
        w.u32(self.line);
        w.flags(&self.flags);
        w.strs(&self.strs);
        w.str(&self.speaker);
        w.str(&self.text);
        w.0
    }

    pub fn decode(b: &[u8]) -> Option<SaveData> {
        let mut r = Reader { b, at: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        Some(SaveData {
            entry: r.str()?,
            entry_flags: r.flags()?,
            entry_strs: r.strs()?,
            scene: r.str()?,
            line: r.u32()?,
            flags: r.flags()?,
            strs: r.strs()?,
            speaker: r.str()?,
            text: r.str()?,
        })
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
    }
    fn flags(&mut self, v: &[(i64, i64)]) {
        self.u32(v.len() as u32);
        for &(k, x) in v {
            self.i64(k);
            self.i64(x);
        }
    }
    fn strs(&mut self, v: &[(i64, String)]) {
        self.u32(v.len() as u32);
        for (k, s) in v {
            self.i64(*k);
            self.str(s);
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn flags(&mut self) -> Option<Vec<(i64, i64)>> {
        (0..self.u32()?)
            .map(|_| Some((self.i64()?, self.i64()?)))
            .collect()
    }
    fn strs(&mut self) -> Option<Vec<(i64, String)>> {
        (0..self.u32()?)
            .map(|_| Some((self.i64()?, self.str()?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = SaveData {
            entry: "00_01".into(),
            entry_flags: vec![(1, 2), (512, -1)],
            entry_strs: vec![(20, "勇希".into())],
            scene: "00_01".into(),
            line: 40,
            flags: vec![(1, 3)],
            strs: vec![],
            speaker: "紗雪".into(),
            text: "「はい」".into(),
        };
        assert_eq!(SaveData::decode(&d.encode()), Some(d));
        assert_eq!(SaveData::decode(b"nope"), None);
    }
}
