//! The compiled scripts (`%04d.gsc` in scr.xfl).
//!
//! ```text
//! file_size:i32 header_size:i32 code_size:i32 table_size:i32 strings_size:i32 ...
//! code (from header_size): op:u16 and its arguments, i32 or i16 each
//! table: i32 per string, its offset into the strings; strings: NUL-ended Shift_JIS
//! ```
//!
//! Places (jumps, the cases of a choice, subroutines) are byte offsets into the code.
//!
//! An i32 argument is a value (its low 16 bits, signed) that is looked up in the variables
//! as many times as its high 16 bits say: 0x0001_1E75 is the variable 7797.
//!
//! Ops with a high byte compute: `dst, a, b` (i16 each) into the temporary `dst`; the op's
//! bits 10-11 say what `a` is (0 a number, 1 a temporary, 2 a variable) and bits 4-7 how many
//! more times it is looked up; bits 8-9 and 0-3 say the same of `b`; the top four bits are
//! the operation (1 `=`, 2 `||`, 3 `&&`, 4 `==`, 5 `>=`, 6 `>`, 7 `<=`, 8 `<`, 9 `!=`, A `+`,
//! B `-`, C `*`, D `/`, E `%`). 0xF1xx (`dst, b`) puts `b` in the temporary `dst`.
//!
//! The argument formats of the ops below 0x100 were found by parsing every script of the
//! games without a byte left over.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg {
    I(i32),
    H(i16),
}

impl Arg {
    pub fn raw(self) -> i32 {
        match self {
            Arg::I(v) => v,
            Arg::H(v) => v as i32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Rec {
    /// where it starts in the code
    pub off: u32,
    pub op: u16,
    pub args: Vec<Arg>,
}

pub struct Script {
    pub recs: Vec<Rec>,
    pub strs: Vec<String>,
}

/// The arguments of an op below 0x100: `i` i32, `h` i16.
pub fn format(op: u16) -> Option<&'static str> {
    if op >= 0x100 {
        return Some(if op & 0xf000 == 0xf000 { "hh" } else { "hhh" });
    }
    Some(match op {
        3 | 4 | 5 | 13 | 16 | 19 | 21 | 44 | 46 | 53 | 65 | 67 | 72 | 74 | 80 | 83 | 94 | 102
        | 105 | 112 | 212 | 230 | 231 => "i",
        8 | 10 | 11 | 17 | 26 | 27 | 50 | 51 | 52 | 55 | 57 | 58 | 68 | 69 | 222 => "",
        9 => "h",
        12
        | 18
        | 20
        | 24
        | 25
        | 29
        | 35
        | 36
        | 37
        | 40
        | 41
        | 42
        | 43
        | 45
        | 47
        | 48
        | 49
        | 61
        | 62
        | 64
        | 92
        | 93
        | 95
        | 96
        | 97
        | 98
        | 101
        | 103
        | 113
        | 114
        | 115
        | 116
        | 117
        | 120
        | 121
        | 132
        | 150..=155
        | 158
        | 159
        | 210
        | 221
        | 223 => "ii",
        28 | 39 | 60 | 73 | 90 | 99 | 100 | 107 | 108 | 110 | 111 | 134 | 136 | 156 | 202 | 213
        | 220 => "iii",
        22 | 23 | 38 | 59 | 63 | 66 | 70 | 71 | 77 | 104 | 130 | 211 => "iiii",
        33 | 34 | 56 | 75 | 91 | 106 | 131 | 135 | 157 | 201 | 225 => "iiiii",
        30 | 32 => "iiiiii",
        81 => "iiiiiii",
        82 => "iiiiii",
        15 => "iiiiiiiiiiii",
        200 => "iiiiiiiiiii",
        14 => "hiiiiiiiiiiiiii",
        255 => "iiiii",
        _ => return None,
    })
}

fn i32_(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Script {
    pub fn parse(b: &[u8]) -> Result<Script, String> {
        let bad = |w: &str| format!("gsc: {w}");
        let hs = i32_(b, 4).ok_or_else(|| bad("cut short"))? as usize;
        let cs = i32_(b, 8).ok_or_else(|| bad("cut short"))? as usize;
        let ts = i32_(b, 12).ok_or_else(|| bad("cut short"))? as usize;
        let ss = i32_(b, 16).ok_or_else(|| bad("cut short"))? as usize;
        let end = hs
            .checked_add(cs)
            .filter(|&e| e <= b.len())
            .ok_or_else(|| bad("bad code size"))?;
        let mut recs = Vec::new();
        let mut at = hs;
        while at + 2 <= end {
            let off = (at - hs) as u32;
            let op = u16::from_le_bytes([b[at], b[at + 1]]);
            at += 2;
            let fmt = format(op).ok_or_else(|| bad(&format!("unknown op {op:#x} at {off:#x}")))?;
            let mut args = Vec::with_capacity(fmt.len());
            for c in fmt.bytes() {
                if c == b'i' {
                    args.push(Arg::I(i32_(b, at).ok_or_else(|| bad("cut short"))?));
                    at += 4;
                } else {
                    let v = b.get(at..at + 2).ok_or_else(|| bad("cut short"))?;
                    args.push(Arg::H(i16::from_le_bytes([v[0], v[1]])));
                    at += 2;
                }
            }
            recs.push(Rec { off, op, args });
        }
        let table = b.get(end..end + ts).unwrap_or(&[]);
        let data = b.get(end + ts..(end + ts + ss).min(b.len())).unwrap_or(&[]);
        let strs = table
            .chunks_exact(4)
            .map(|c| {
                let o = i32::from_le_bytes(c.try_into().unwrap()).max(0) as usize;
                let s = data.get(o..).unwrap_or(&[]);
                let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                encoding_rs::SHIFT_JIS.decode(&s[..n]).0.into_owned()
            })
            .collect();
        Ok(Script { recs, strs })
    }

    /// The record at a code offset (or the first after it).
    pub fn at(&self, off: u32) -> usize {
        match self.recs.binary_search_by_key(&off, |r| r.off) {
            Ok(i) | Err(i) => i,
        }
    }

    pub fn s(&self, i: i32) -> &str {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.strs.get(i))
            .map_or("", |s| s.as_str())
    }
}
