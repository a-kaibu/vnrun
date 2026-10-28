//! The words of the game: tblstr.arc holds entries of (u32 kind, u32 length, the text with
//! every byte inverted, ended by a 0 byte that is not inverted, and padded), kind 0 a line and 2 a speaker's name;
//! tblstr.ari is a u32 count and then the offsets of the lines, so line `i` is at the i-th u32
//! (the count being the 0th). The scripts name lines by number and names by offset.

pub struct Text {
    arc: Vec<u8>,
    ari: Vec<u8>,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Text {
    pub fn new(arc: Vec<u8>, ari: Vec<u8>) -> Text {
        Text { arc, ari }
    }

    /// The entry at byte `offset` of tblstr.arc.
    pub fn at(&self, offset: usize) -> Option<String> {
        let len = u32le(&self.arc, offset + 4)? as usize;
        let raw = self.arc.get(offset + 8..offset + 8 + len)?;
        let raw: Vec<u8> = raw.iter().take_while(|&&b| b != 0).map(|b| !b).collect();
        let s = encoding_rs::SHIFT_JIS.decode(&raw).0;
        Some(s.replace('\r', "").trim_end_matches('\n').to_string())
    }

    /// Line `i` of the scripts.
    pub fn line(&self, i: usize) -> Option<String> {
        if i == 0 {
            return None;
        }
        self.at(u32le(&self.ari, i * 4)? as usize)
    }
}
