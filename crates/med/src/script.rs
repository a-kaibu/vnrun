//! The compiled scripts of md_scr.med.
//!
//! ```text
//! size:u32 (file size - 16) code:u32 counts:u32 (labels << 16 | strings) last_line:u32
//! code (code - 1 bytes): records  line:u16 n:u8 op:u8 args:[u8; n - 1], then a 0 byte
//! labels: u16 per label, the label's place (a byte offset into the code)
//! strings: NUL-ended Shift_JIS, the script's strings and then the labels' names
//! ```
//!
//! Places in the code (jumps, labels, the cases of a SELECT) are byte offsets from the start of
//! the code. The data scripts (`_BGSET`, `_SPRSET`, `_MAP`, ...) hold no code: their strings
//! are the lines of their source.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Rec {
    /// where the record starts in the code
    pub off: u32,
    pub op: u8,
    pub args: Vec<u8>,
}

impl Rec {
    pub fn u8(&self, at: usize) -> u8 {
        self.args.get(at).copied().unwrap_or(0)
    }

    pub fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.u8(at), self.u8(at + 1)])
    }

    pub fn i16(&self, at: usize) -> i16 {
        self.u16(at) as i16
    }
}

pub struct Script {
    pub recs: Vec<Rec>,
    pub strs: Vec<String>,
    /// by upper-case name: the record index
    labels: HashMap<String, usize>,
    by_off: HashMap<u32, usize>,
    /// for each case record (10) of a SELECT, CLICKMAP or SWITCH: where its block ends
    pub case_exit: HashMap<usize, usize>,
}

fn u32_(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Script {
    pub fn parse(b: &[u8]) -> Result<Script, String> {
        let bad = || "script: cut short".to_string();
        let code = u32_(b, 4).ok_or_else(bad)? as usize;
        let counts = u32_(b, 8).ok_or_else(bad)?;
        let (nstr, nlab) = ((counts & 0xffff) as usize, (counts >> 16) as usize);
        let end = 0x10 + code.saturating_sub(1);
        if end > b.len() {
            return Err(bad());
        }
        let mut recs = Vec::new();
        let mut by_off = HashMap::new();
        let mut at = 0x10;
        while at + 3 <= end {
            let n = b[at + 2] as usize;
            if n == 0 || at + 3 + n > end {
                break;
            }
            let off = (at - 0x10) as u32;
            by_off.insert(off, recs.len());
            recs.push(Rec {
                off,
                op: b[at + 3],
                args: b[at + 4..at + 3 + n].to_vec(),
            });
            at += 3 + n;
        }
        let mut at = 0x10 + code;
        let mut places = Vec::with_capacity(nlab);
        for _ in 0..nlab {
            let p = b.get(at..at + 2).ok_or_else(bad)?;
            places.push(u16::from_le_bytes([p[0], p[1]]) as u32);
            at += 2;
        }
        let mut parts = b.get(at..).unwrap_or(&[]).split(|&c| c == 0);
        let mut strs = Vec::with_capacity(nstr);
        for _ in 0..nstr {
            let s = parts.next().unwrap_or(&[]);
            strs.push(encoding_rs::SHIFT_JIS.decode(s).0.into_owned());
        }
        let mut s = Script {
            recs,
            strs,
            labels: HashMap::new(),
            by_off,
            case_exit: HashMap::new(),
        };
        for p in places {
            let name = parts.next().unwrap_or(&[]);
            let name = encoding_rs::SHIFT_JIS.decode(name).0.to_uppercase();
            let i = s.at(p);
            s.labels.insert(name, i);
        }
        for i in 0..s.recs.len() {
            if let Some((cases, _)) = s.block(i)
                && let Some(exit) = s.recs.get(i + 1).filter(|r| r.op == 8).map(|r| r.u16(1))
            {
                let exit = s.at(exit as u32);
                for c in cases {
                    s.case_exit.insert(c, exit);
                }
            }
        }
        Ok(s)
    }

    /// The record at a code offset (or the first after it).
    pub fn at(&self, off: u32) -> usize {
        if let Some(&i) = self.by_off.get(&off) {
            return i;
        }
        self.recs
            .iter()
            .position(|r| r.off >= off)
            .unwrap_or(self.recs.len())
    }

    pub fn label(&self, name: &str) -> Option<usize> {
        self.labels.get(&name.to_uppercase()).copied()
    }

    pub fn s(&self, i: u16) -> &str {
        self.strs.get(i as usize).map_or("", |s| s.as_str())
    }

    /// A SELECT (33), CLICKMAP (34) or SWITCH (42): the records of its cases and of its end.
    pub fn block(&self, i: usize) -> Option<(Vec<usize>, usize)> {
        let r = self.recs.get(i)?;
        let start = match r.op {
            33 | 34 => 2,
            42 => 0,
            _ => return None,
        };
        let n = r.u8(start) as usize;
        if r.args.len() < start + 1 + 2 * (n + 1) {
            return None;
        }
        let cases = (0..n)
            .map(|k| self.at(r.u16(start + 1 + 2 * k) as u32))
            .collect();
        let end = self.at(r.u16(start + 1 + 2 * n) as u32);
        Some((cases, end))
    }
}

/// Values of an expression (ops 6, 10, 29): up to the 0xff that ends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    /// class byte, index
    Var(u8, u8),
    Num(i32),
    /// 0xee + - * / & | % = <> > >= < <= && || ( ) (0xfe)
    Op(u8),
}

/// The tokens of an expression and where it ended.
pub fn tokens(b: &[u8]) -> (Vec<Tok>, usize) {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let c = b[at];
        match c {
            0xff => return (out, at + 1),
            0xeb => {
                out.push(Tok::Num(b.get(at + 1).copied().unwrap_or(0) as i32));
                at += 2;
            }
            0xec => {
                let v = u16::from_le_bytes([
                    b.get(at + 1).copied().unwrap_or(0),
                    b.get(at + 2).copied().unwrap_or(0),
                ]);
                out.push(Tok::Num(v as i16 as i32));
                at += 3;
            }
            0xed => {
                let mut v = [0u8; 4];
                for (k, x) in v.iter_mut().enumerate() {
                    *x = b.get(at + 1 + k).copied().unwrap_or(0);
                }
                out.push(Tok::Num(i32::from_le_bytes(v)));
                at += 5;
            }
            0xee..=0xfe => {
                out.push(Tok::Op(c));
                at += 1;
            }
            _ => {
                out.push(Tok::Var(c, b.get(at + 1).copied().unwrap_or(0)));
                at += 2;
            }
        }
    }
    (out, at)
}

/// Evaluates tokens with `var` giving variables' values. A lone `=` compares.
pub fn eval(t: &[Tok], var: &mut dyn FnMut(u8, u8) -> i32) -> i32 {
    let mut p = Parser { t, at: 0 };
    p.expr(0, var)
}

struct Parser<'a> {
    t: &'a [Tok],
    at: usize,
}

const ADD: u8 = 0xee;
const SUB: u8 = 0xef;
const MUL: u8 = 0xf0;
const DIV: u8 = 0xf1;
const AND: u8 = 0xf2;
const OR: u8 = 0xf3;
const MOD: u8 = 0xf4;
const EQ: u8 = 0xf5;
const NE: u8 = 0xf6;
const GT: u8 = 0xf7;
const GE: u8 = 0xf8;
const LT: u8 = 0xf9;
const LE: u8 = 0xfa;
const LAND: u8 = 0xfb;
const LOR: u8 = 0xfc;
const OPEN: u8 = 0xfd;
const CLOSE: u8 = 0xfe;

fn prec(op: u8) -> u8 {
    match op {
        LOR => 1,
        LAND => 2,
        EQ | NE | GT | GE | LT | LE => 3,
        AND | OR => 4,
        ADD | SUB => 5,
        MUL | DIV | MOD => 6,
        _ => 0,
    }
}

impl Parser<'_> {
    fn atom(&mut self, var: &mut dyn FnMut(u8, u8) -> i32) -> i32 {
        match self.t.get(self.at).copied() {
            Some(Tok::Num(n)) => {
                self.at += 1;
                n
            }
            Some(Tok::Var(c, i)) => {
                self.at += 1;
                var(c, i)
            }
            Some(Tok::Op(OPEN)) => {
                self.at += 1;
                let v = self.expr(0, var);
                if self.t.get(self.at) == Some(&Tok::Op(CLOSE)) {
                    self.at += 1;
                }
                v
            }
            Some(Tok::Op(SUB)) => {
                self.at += 1;
                -self.atom(var)
            }
            _ => {
                self.at += 1;
                0
            }
        }
    }

    fn expr(&mut self, min: u8, var: &mut dyn FnMut(u8, u8) -> i32) -> i32 {
        let mut l = self.atom(var);
        while let Some(Tok::Op(op)) = self.t.get(self.at).copied() {
            let p = prec(op);
            if p == 0 || p <= min {
                break;
            }
            self.at += 1;
            if op == EQ && self.t.get(self.at) == Some(&Tok::Op(EQ)) {
                // "==" is two of them
                self.at += 1;
            }
            let r = self.expr(p, var);
            l = match op {
                ADD => l.wrapping_add(r),
                SUB => l.wrapping_sub(r),
                MUL => l.wrapping_mul(r),
                DIV => l.checked_div(r).unwrap_or(0),
                MOD => l.checked_rem(r).unwrap_or(0),
                AND => l & r,
                OR => l | r,
                EQ => (l == r) as i32,
                NE => (l != r) as i32,
                GT => (l > r) as i32,
                GE => (l >= r) as i32,
                LT => (l < r) as i32,
                LE => (l <= r) as i32,
                LAND => (l != 0 && r != 0) as i32,
                LOR => (l != 0 || r != 0) as i32,
                _ => l,
            };
        }
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(b: &[u8], v: i32) -> i32 {
        let (t, _) = tokens(b);
        eval(&t, &mut |_, _| v)
    }

    #[test]
    fn expressions() {
        // var + 1
        assert_eq!(run(&[0x00, 0x20, 0xee, 0xeb, 0x01, 0xff], 4), 5);
        // var <= 2
        assert_eq!(run(&[0x00, 0x20, 0xfa, 0xeb, 0x02, 0xff], 2), 1);
        assert_eq!(run(&[0x00, 0x20, 0xfa, 0xeb, 0x02, 0xff], 3), 0);
        // var == 1 || var == 1, "==" written as two '='
        let b = [
            0xcf, 0x10, 0xf5, 0xf5, 0xeb, 0x01, 0xfc, 0xd7, 0x16, 0xf5, 0xeb, 0x01, 0xff,
        ];
        assert_eq!(run(&b, 1), 1);
        assert_eq!(run(&b, 0), 0);
        // a 32-bit literal
        assert_eq!(run(&[0xed, 0x77, 0x77, 0x77, 0x01, 0xff], 0), 0x0177_7777);
    }
}
