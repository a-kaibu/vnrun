//! Scenario scripts (.ws2): a stream of commands, each an opcode byte and its arguments. The
//! bytes are stored rotated right by 2 in some builds (always, in the ones seen so far).
//!
//! Argument types, from the exe's table ([`crate::exe`]):
//!
//! | type | value |
//! |---|---|
//! | 0 | u8 |
//! | 1 | u16 |
//! | 2 | i16 |
//! | 3 | u32 |
//! | 4 | i32 (offsets in the script) |
//! | 5 | f32 |
//! | 6, 9, 10 | string up to NUL (text, variable name, file name); the NUL is a type 8. Shift_JIS, or UTF-16LE in 2.1 |
//! | 7 | array: a u8 count, then that many of the element type (the next type byte) |
//! | 8 | one byte skipped (a string's NUL) |

use crate::exe::Formats;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i64),
    Num(f32),
    Str(String),
    List(Vec<Arg>),
}

impl Arg {
    pub fn int(&self) -> i64 {
        match self {
            Arg::Int(i) => *i,
            Arg::Num(n) => *n as i64,
            Arg::Str(s) => s.trim().parse().unwrap_or(0),
            Arg::List(_) => 0,
        }
    }
    pub fn num(&self) -> f32 {
        match self {
            Arg::Num(n) => *n,
            a => a.int() as f32,
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Arg::Str(s) => s,
            _ => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ins {
    pub at: usize,
    pub op: u8,
    pub args: Vec<Arg>,
    /// where the next command starts
    pub next: usize,
}

impl Ins {
    pub fn arg(&self, i: usize) -> &Arg {
        static NONE: Arg = Arg::Int(0);
        self.args.get(i).unwrap_or(&NONE)
    }
}

pub struct Script {
    pub name: String,
    pub code: Vec<u8>,
}

impl Script {
    pub fn new(name: &str, raw: &[u8]) -> Script {
        Script {
            name: name.to_string(),
            code: raw.iter().map(|b| b.rotate_right(2)).collect(),
        }
    }

    /// The command at `at`.
    pub fn read(&self, fm: &Formats, at: usize) -> Result<Ins, String> {
        let b = &self.code;
        let op = *b.get(at).ok_or("past the end of the script")?;
        let types = fm
            .types
            .get(op as usize)
            .and_then(|t| t.as_ref())
            .ok_or_else(|| format!("{}:{at:#x}: unknown command {op:#04x}", self.name))?;
        let mut p = at + 1;
        let w = fm.utf16;
        let nul = if w { 2 } else { 1 };
        if op == 0x0f {
            return self.choices(at, w);
        }
        let mut args = Vec::with_capacity(types.len());
        let mut i = 0;
        while i < types.len() {
            let t = types[i];
            if t == 7 {
                let n = *b.get(p).ok_or("cut short")? as usize;
                p += 1;
                let et = *types.get(i + 1).ok_or("array without element type")?;
                let mut list = Vec::with_capacity(n.min(256));
                for _ in 0..n {
                    list.push(read_arg(b, &mut p, et, w)?);
                    if matches!(et, 6 | 9 | 10) {
                        p += nul;
                    }
                }
                args.push(Arg::List(list));
                i += 2;
                continue;
            }
            if t == 8 {
                p += nul;
            } else {
                args.push(read_arg(b, &mut p, t, w)?);
            }
            i += 1;
        }
        if p > b.len() {
            return Err(format!(
                "{}:{at:#x}: command {op:#04x} is cut short",
                self.name
            ));
        }
        Ok(Ins {
            at,
            op,
            args,
            next: p,
        })
    }

    /// Command 0x0f (show choices) has a layout the table can't express: a u8 count, then per
    /// choice a u16 id, the text, four bytes whose last says what follows (6: a u32 offset to
    /// jump to, 7: a script name, other: nothing). Each choice becomes one [`Arg::List`].
    fn choices(&self, at: usize, w: bool) -> Result<Ins, String> {
        let b = &self.code;
        let mut p = at + 1;
        let n = *b.get(p).ok_or("cut short")? as usize;
        p += 1;
        let mut args = Vec::with_capacity(n);
        for _ in 0..n {
            let nul = if w { 2 } else { 1 };
            let mut c = vec![read_arg(b, &mut p, 1, w)?, read_arg(b, &mut p, 6, w)?];
            p += nul;
            for _ in 0..4 {
                c.push(read_arg(b, &mut p, 0, w)?);
            }
            match c[5].int() {
                6 => c.push(read_arg(b, &mut p, 3, w)?),
                7 => {
                    c.push(read_arg(b, &mut p, 6, w)?);
                    p += nul;
                }
                _ => {}
            }
            args.push(Arg::List(c));
        }
        if p > b.len() {
            return Err(format!("{}:{at:#x}: command 0x0f is cut short", self.name));
        }
        Ok(Ins {
            at,
            op: 0x0f,
            args,
            next: p,
        })
    }
}

/// One argument; `w`: strings are UTF-16LE.
fn read_arg(b: &[u8], p: &mut usize, t: u8, w: bool) -> Result<Arg, String> {
    let take = |p: &mut usize, n: usize| -> Result<&[u8], String> {
        let s = b.get(*p..*p + n).ok_or("argument is cut short")?;
        *p += n;
        Ok(s)
    };
    Ok(match t {
        0 => Arg::Int(take(p, 1)?[0] as i64),
        1 => Arg::Int(u16::from_le_bytes(take(p, 2)?.try_into().unwrap()) as i64),
        2 => Arg::Int(i16::from_le_bytes(take(p, 2)?.try_into().unwrap()) as i64),
        3 => Arg::Int(u32::from_le_bytes(take(p, 4)?.try_into().unwrap()) as i64),
        4 => Arg::Int(i32::from_le_bytes(take(p, 4)?.try_into().unwrap()) as i64),
        5 => Arg::Num(f32::from_le_bytes(take(p, 4)?.try_into().unwrap())),
        6 | 9 | 10 if w => {
            let n = b
                .get(*p..)
                .and_then(|s| s.chunks_exact(2).position(|c| c == [0, 0]))
                .ok_or("string is cut short")?;
            let u: Vec<u16> = take(p, 2 * n)?
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Arg::Str(String::from_utf16_lossy(&u))
        }
        6 | 9 | 10 => {
            let n = b
                .get(*p..)
                .and_then(|s| s.iter().position(|&c| c == 0))
                .ok_or("string is cut short")?;
            Arg::Str(crate::sjis(take(p, n)?))
        }
        t => return Err(format!("argument type {t}")),
    })
}
