//! MSC scripts (BB.exe: loader 0x406590, step 0x40e3d0, value reader 0x4125b0).
//!
//! ```text
//! 0:u16  code_start:u32  labels_size:u32  labels: [kind:u8 id:u16 offset:u32] (7 bytes each)
//! globals_size:u32  globals: same records   code (to the end)
//! ```
//!
//! Label ids count from 1 and offsets are from the start of the code. Local labels are the
//! targets of jumps in the script; global labels are where other scripts start threads
//! (0.20) and are known once the script was loaded.
//!
//! An instruction is a category byte (0..=6) and a command byte, then its operands:
//! B (u8), W (i16), S (len:u16 + Shift-JIS bytes) and V, a value: kind:u8 n:i16 with kind
//! 0 the number, 1 global variable n, 2 system variable n, 3 a random number below n,
//! 4 the base sprite (set by 1.02).
//!
//! The 2011 generation (すきま桜, sukima_trial2.exe: step 0x45ae20, value reader 0x44f970,
//! string reader 0x460ed0) widens the fields: label records are [kind:u8 id:u32 offset:u32]
//! (9 bytes), W is an i32, S has a u32 length and V is kind:u8 n:i32. V kinds 7 and 8 there are
//! the pointer position and 9.. other engine state (mostly the config screen's settings). It
//! adds commands (marked v2 in [`syntax`]) and a second byte to 0.20.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Num(i32),
    Var(i32),
    Sys(i32),
    Rand(i32),
    Base,
    /// engine state (v2): 7 pointer x, 8 pointer y, others the config screen's settings
    Special(u8),
}

#[derive(Clone, PartialEq)]
pub enum Arg {
    B(u8),
    W(i32),
    V(Val),
    S(Vec<u8>),
}

impl std::fmt::Debug for Arg {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Arg::B(b) => write!(f, "{b}"),
            Arg::W(w) => write!(f, "w{w}"),
            Arg::V(v) => write!(f, "{v:?}"),
            Arg::S(s) => write!(f, "{:?}", encoding_rs::SHIFT_JIS.decode(s).0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ins {
    pub cat: u8,
    pub op: u8,
    pub args: Vec<Arg>,
    /// the offset of the next instruction
    pub next: usize,
}

pub struct Script {
    pub name: String,
    pub code: Vec<u8>,
    pub labels: HashMap<u32, usize>,
    pub globals: HashMap<u32, usize>,
    /// the 2011 layout (wide fields)
    pub v2: bool,
}

/// The operands of each command, from the handlers of BB.exe (0x40ea90 and the tables
/// after it) and, for `v2`, of sukima_trial2.exe (categories at 0x4597e0, 0x4580b0,
/// 0x4500d0, 0x451680, 0x44fd20, 0x455c60, 0x451b10).
pub fn syntax(cat: u8, op: u8, v2: bool) -> Option<&'static str> {
    if v2 {
        let s = match (cat, op) {
            (0, 0x20) => Some("VVBB"),
            (0, 0x33 | 0x34) => Some("V"),
            (0, 0x35) => Some(""),
            (0, 0x36) => Some("B"),
            (0, 0x37) => Some("BVV"),
            (1, 0x18) => Some("S"),
            (1, 0x19) => Some("SV"),
            (1, 0x1a) => Some("BB"),
            (1, 0x1b) => Some("BV"),
            (1, 0x1c | 0x1d) => Some("BVV"),
            (1, 0x1e) => Some("SS"),
            (1, 0x1f) => Some("SSS"),
            (2, 0x17) => Some("VB"),
            (5, 0x05) => Some("B"),
            (5, 0x06) => Some(""),
            (5, 0x07) => Some("BB"),
            (6, 0x0a) => Some("S"),
            (6, 0x0b) => Some("BVV"),
            (6, 0x0c) => Some("VVVV"),
            (6, 0x0d) => Some(""),
            (6, 0x0e) => Some("VV"),
            _ => None,
        };
        if s.is_some() {
            return s;
        }
    }
    Some(match (cat, op) {
        (0, 0x00) => "BWBV",
        (0, 0x01) => "VBVBV",
        (0, 0x02 | 0x03 | 0x10 | 0x11 | 0x17) => "V",
        (0, 0x05) => "VB",
        (0, 0x07 | 0x0a | 0x16) => "B",
        (0, 0x08) => "S",
        (0, 0x20) => "VVB",
        (0, 0x04 | 0x06 | 0x09 | 0x0b..=0x0f | 0x12..=0x14 | 0x30..=0x32) => "",
        (1, 0x00) => "S",
        (1, 0x01 | 0x04 | 0x15) => "BS",
        (1, 0x02 | 0x0f | 0x11 | 0x14 | 0x16) => "V",
        (1, 0x03) => "SSS",
        (1, 0x05..=0x07 | 0x0a) => "VV",
        (1, 0x08 | 0x12) => "VVVVVVV",
        (1, 0x09) => "SSSB",
        (1, 0x0b) => "VVS",
        (1, 0x0c) => "BV",
        (1, 0x0d) => "VSS",
        (1, 0x0e) => "BVVVV",
        (1, 0x10) => "BVVV",
        (1, 0x13) => "VVVVV",
        (1, 0x17) => "SS",
        (2, 0x00) => "VS",
        (2, 0x01 | 0x05 | 0x0a | 0x10 | 0x11) => "VVV",
        (2, 0x02) => "VB",
        (2, 0x03) => "VVVVV",
        (2, 0x04 | 0x06 | 0x07 | 0x0d..=0x0f | 0x13) => "VV",
        (2, 0x08 | 0x0b | 0x15) => "VVVV",
        (2, 0x09) => "VVVVVV",
        (2, 0x0c) => "VVVVB",
        (2, 0x12) => "VVVVVS",
        (2, 0x14) => "VSVVVB",
        (2, 0x16) => "VSVVVV",
        (3, 0x00) => "VSVB",
        (3, 0x01) => "VVB",
        (3, 0x02) => "V",
        (3, 0x03) => "SB",
        (4, 0x00) => "VBV",
        (4, 0x01 | 0x02) => "VB",
        (4, 0x03 | 0x04) => "VV",
        (5, 0x00) => "VS",
        (5, 0x01) => "VVVVVVVV",
        (5, 0x02) => "VVVVVVV",
        (5, 0x03 | 0x04) => "VV",
        (6, 0x00) => "SBV",
        (6, 0x01 | 0x06 | 0x08 | 0x09) => "V",
        (6, 0x02) => "S",
        (6, 0x03 | 0x07) => "",
        (6, 0x04) => "VS",
        (6, 0x05) => "VB",
        _ => return None,
    })
}

fn u32_(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn table(b: &[u8], at: usize, size: usize, v2: bool) -> Option<HashMap<u32, usize>> {
    let mut m = HashMap::new();
    let w = if v2 { 9 } else { 7 };
    for i in 0..size / w {
        let p = at + i * w;
        let id = if v2 {
            u32_(b, p + 1)? as u32
        } else {
            u16::from_le_bytes([*b.get(p + 1)?, *b.get(p + 2)?]) as u32
        };
        m.insert(id, u32_(b, p + w - 4)?);
    }
    Some(m)
}

/// A signed number: i32 in the 2011 layout, i16 before; with its width.
fn int(c: &[u8], p: usize, wide: bool) -> Option<(i32, usize)> {
    Some(if wide {
        (i32::from_le_bytes(c.get(p..p + 4)?.try_into().ok()?), 4)
    } else {
        (i16::from_le_bytes([*c.get(p)?, *c.get(p + 1)?]) as i32, 2)
    })
}

impl Script {
    pub fn parse(name: &str, b: &[u8], v2: bool) -> Option<Script> {
        let start = u32_(b, 2)?;
        let n1 = u32_(b, 6)?;
        let labels = table(b, 0xa, n1, v2)?;
        let n2 = u32_(b, 0xa + n1)?;
        let globals = table(b, 0xe + n1, n2, v2)?;
        if 0xe + n1 + n2 != start {
            return None;
        }
        Some(Script {
            name: name.to_string(),
            code: b.get(start..)?.to_vec(),
            labels,
            globals,
            v2,
        })
    }

    /// Whether every instruction decodes, to tell the two layouts apart.
    pub fn decodes(&self) -> bool {
        let mut pc = 0;
        while pc < self.code.len() {
            match self.decode(pc) {
                Some(i) => pc = i.next,
                None => return false,
            }
        }
        pc == self.code.len()
    }

    pub fn decode(&self, pc: usize) -> Option<Ins> {
        let c = &self.code;
        let (cat, op) = (*c.get(pc)?, *c.get(pc + 1)?);
        let syn = syntax(cat, op, self.v2)?;
        let wide = self.v2;
        let mut p = pc + 2;
        let mut args = Vec::with_capacity(syn.len());
        for ch in syn.bytes() {
            match ch {
                b'B' => {
                    args.push(Arg::B(*c.get(p)?));
                    p += 1;
                }
                b'W' => {
                    let (n, w) = int(c, p, wide)?;
                    args.push(Arg::W(n));
                    p += w;
                }
                b'V' => {
                    let (n, w) = int(c, p + 1, wide)?;
                    args.push(Arg::V(match *c.get(p)? {
                        0 => Val::Num(n),
                        1 => Val::Var(n),
                        2 => Val::Sys(n),
                        3 => Val::Rand(n),
                        4 => Val::Base,
                        k if wide && k <= 0x80 => Val::Special(k),
                        _ => return None,
                    }));
                    p += 1 + w;
                }
                _ => {
                    let (n, w) = if wide {
                        (u32_(c, p)?, 4)
                    } else {
                        (u16::from_le_bytes([*c.get(p)?, *c.get(p + 1)?]) as usize, 2)
                    };
                    args.push(Arg::S(c.get(p + w..p + w + n)?.to_vec()));
                    p += w + n;
                }
            }
        }
        Some(Ins {
            cat,
            op,
            args,
            next: p,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        // one local label (id 1 at 0), no globals; code: 2.01 sprite 5 at (Num -3, Var 2)
        let mut b = vec![
            0, 0, 0x15, 0, 0, 0, 7, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        b.extend_from_slice(&[2, 1, 0, 5, 0, 0, 0xfd, 0xff, 1, 2, 0]);
        let s = Script::parse("t", &b, false).unwrap();
        assert!(s.decodes());
        assert_eq!(s.labels[&1], 0);
        let i = s.decode(0).unwrap();
        assert_eq!((i.cat, i.op, i.next), (2, 1, 11));
        assert_eq!(i.args[1], Arg::V(Val::Num(-3)));
        assert_eq!(i.args[2], Arg::V(Val::Var(2)));
    }

    #[test]
    fn decode_v2() {
        // one 9-byte label (id 1 at 0), no globals; code: 1.00 S "ab", 2.01 V V V
        let mut b = vec![0, 0, 0x17, 0, 0, 0, 9, 0, 0, 0];
        b.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&[1, 0, 2, 0, 0, 0, b'a', b'b']);
        b.extend_from_slice(&[2, 1, 0, 5, 0, 0, 0, 7, 0, 0, 0, 0, 1, 2, 0, 0, 0]);
        let s = Script::parse("t", &b, true).unwrap();
        assert!(s.decodes());
        assert_eq!(s.labels[&1], 0);
        let i = s.decode(8).unwrap();
        assert_eq!(i.args[1], Arg::V(Val::Special(7)));
        assert_eq!(i.args[2], Arg::V(Val::Var(2)));
        assert!(Script::parse("t", &b, false).is_none_or(|s| !s.decodes()));
    }
}
