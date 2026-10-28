//! "NEKOSDK_ADVSCRIPT2" scripts (scr\*.txt and flow\*.txt, despite the name binary): the
//! signature and a NUL, u32 count, then per record 53 i32 and 33 strings (u32 length with the
//! NUL, Shift_JIS). The records are the scenario editor's rows: int 0 is the row's id, int 3 the
//! command, ints 4.. its numbers, int 36 the id of the next row and int 37 a second way on (for
//! a test). String 0 is the editor's summary, strings 1.. the command's names and texts.

use std::collections::HashMap;

pub const INTS: usize = 53;
pub const STRS: usize = 33;

#[derive(Clone, Debug)]
pub struct Rec {
    pub ints: [i32; INTS],
    pub strs: Vec<String>,
}

impl Rec {
    pub fn id(&self) -> i32 {
        self.ints[0]
    }
    pub fn cmd(&self) -> i32 {
        self.ints[3]
    }
    /// Command number `i` (counted from 0 after the command).
    pub fn n(&self, i: usize) -> i32 {
        self.ints.get(4 + i).copied().unwrap_or(0)
    }
    pub fn next(&self) -> i32 {
        self.ints[36]
    }
    pub fn alt(&self) -> i32 {
        self.ints[37]
    }
    pub fn s(&self, i: usize) -> &str {
        self.strs.get(i).map_or("", String::as_str)
    }
}

pub struct Script {
    pub recs: Vec<Rec>,
    by_id: HashMap<i32, usize>,
}

impl Script {
    pub fn parse(b: &[u8]) -> Result<Script, String> {
        const SIG: &[u8] = b"NEKOSDK_ADVSCRIPT2\0";
        if !b.starts_with(SIG) {
            return Err("not an ADV script".into());
        }
        let mut at = SIG.len();
        let u32_at = |at: usize| -> Result<u32, String> {
            b.get(at..at + 4)
                .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
                .ok_or_else(|| "script cut short".to_string())
        };
        let n = u32_at(at)? as usize;
        at += 4;
        let mut recs = Vec::with_capacity(n.min(100_000));
        for _ in 0..n {
            let mut ints = [0i32; INTS];
            for v in ints.iter_mut() {
                *v = u32_at(at)? as i32;
                at += 4;
            }
            let mut strs = Vec::with_capacity(STRS);
            for _ in 0..STRS {
                let l = u32_at(at)? as usize;
                at += 4;
                let s = b.get(at..at + l).ok_or("script cut short")?;
                at += l;
                let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                strs.push(
                    encoding_rs::SHIFT_JIS
                        .decode(&s[..end])
                        .0
                        .replace("\r\n", "\n"),
                );
            }
            recs.push(Rec { ints, strs });
        }
        let by_id = recs.iter().enumerate().map(|(i, r)| (r.id(), i)).collect();
        Ok(Script { recs, by_id })
    }

    pub fn get(&self, id: i32) -> Option<&Rec> {
        self.by_id.get(&id).map(|&i| &self.recs[i])
    }

    /// The first row (the editor's start mark).
    pub fn start(&self) -> Option<&Rec> {
        self.recs
            .iter()
            .find(|r| r.cmd() == 0)
            .or(self.recs.first())
    }
}
