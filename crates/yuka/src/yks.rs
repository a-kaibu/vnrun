//! YKS001 scripts. The header (0x30 bytes): "YKS001", whether the data is encrypted (XOR 0xAA),
//! the header size, then where the code is and its number of words, where the element table is
//! and its number of elements, where the data is and its size.
//!
//! The code is a list of element numbers. An element (16 bytes: kind and three words that point
//! into the data) is a function name (0), a control word (1: a label, `{`, `}`, `else` or an
//! operator, with the code position it links to), an integer (4), a string (5), a numeric or
//! string variable (8, 9: its kind like "Flag" or "GlobalString" and its number) or a temporary
//! (10). A call is the function, its number of arguments (a plain number, not an element) and
//! the arguments; an assignment is a variable, the function "=", the number of operands and the
//! operands with operators between them, or 0 operands and then the call whose result it takes.
//! An `if` call is followed by a `{` that links to its `}`, which links back; `else` and its
//! block may follow. The integers that give a variable its number are data cells a script can
//! write to (an integer element as the target of "="), to reach variables by a computed number.

#[derive(Debug, Clone)]
pub enum Elem {
    Func(String),
    Ctrl(String, i32),
    /// an integer: its cell in the data
    Int(u32),
    Str(String),
    /// a variable: its kind and the cell with its number
    Var(String, u32),
    Tmp,
    Other,
}

pub struct Script {
    pub name: String,
    pub code: Vec<u32>,
    pub elems: Vec<Elem>,
    /// the data, decrypted (the integer cells change as the script runs)
    pub data: Vec<u8>,
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        if !b.starts_with(b"YKS001") || b.len() < 0x30 {
            return Err(format!("{name}: not a YKS001 script"));
        }
        let u = |o: usize| -> Result<usize, String> {
            b.get(o..o + 4)
                .map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize)
                .ok_or_else(|| format!("{name}: cut short"))
        };
        let enc = b[6] != 0;
        let (co, cn, io, n, dof, ds) = (u(0x10)?, u(0x14)?, u(0x18)?, u(0x1c)?, u(0x20)?, u(0x24)?);
        let short = || format!("{name}: cut short");
        let code: Vec<u32> = b
            .get(co..co + cn * 4)
            .ok_or_else(short)?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let mut data = b.get(dof..dof + ds).ok_or_else(short)?.to_vec();
        if enc {
            for c in &mut data {
                *c ^= 0xaa;
            }
        }
        let table = b.get(io..io + n * 16).ok_or_else(short)?;
        let cstr = |o: usize| -> String {
            let s = data.get(o..).unwrap_or(&[]);
            let e = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            nexas::sjis::decode(&s[..e])
        };
        let int = |o: usize| -> i32 {
            data.get(o..o + 4)
                .map_or(0, |x| i32::from_le_bytes(x.try_into().unwrap()))
        };
        let elems = table
            .chunks_exact(16)
            .map(|e| {
                let w = |k: usize| u32::from_le_bytes(e[k * 4..k * 4 + 4].try_into().unwrap());
                let (a, bb, c) = (w(1) as usize, w(2) as usize, w(3));
                match w(0) {
                    0 => Elem::Func(cstr(a)),
                    1 => Elem::Ctrl(cstr(a), int(bb)),
                    4 => Elem::Int(bb as u32),
                    5 => Elem::Str(cstr(bb)),
                    8 | 9 => Elem::Var(cstr(a), c),
                    10 => Elem::Tmp,
                    _ => Elem::Other,
                }
            })
            .collect();
        Ok(Script {
            name: name.to_string(),
            code,
            elems,
            data,
        })
    }

    pub fn int(&self, cell: u32) -> i32 {
        let o = cell as usize;
        self.data
            .get(o..o + 4)
            .map_or(0, |x| i32::from_le_bytes(x.try_into().unwrap()))
    }

    pub fn set_int(&mut self, cell: u32, v: i32) {
        let o = cell as usize;
        if let Some(x) = self.data.get_mut(o..o + 4) {
            x.copy_from_slice(&v.to_le_bytes());
        }
    }

    pub fn elem(&self, pos: usize) -> Option<&Elem> {
        self.code.get(pos).and_then(|&i| self.elems.get(i as usize))
    }

    /// The code position of a label.
    pub fn label(&self, name: &str) -> Option<usize> {
        self.code
            .iter()
            .enumerate()
            .find_map(|(p, &i)| match self.elems.get(i as usize) {
                Some(Elem::Ctrl(n, _)) if n == name => Some(p),
                _ => None,
            })
    }
}
