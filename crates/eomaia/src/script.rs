//! The compiled scripts ("SCRE"): .script, .lib and .package files.
//!
//! "SCRE", a version, a count and the offset (0x14) of a table of `count` u32 block offsets;
//! the code follows the table, and every jump names an offset from the code's start. Numbers
//! in the code are big-endian, strings UTF-8 ending in NUL.

use std::collections::HashMap;

pub struct Script {
    pub name: String,
    pub code: Vec<u8>,
    /// where the code starts (jumps are relative to it)
    pub base: usize,
}

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// 0x00: the end of a script (or of a choice's text)
    End,
    /// 0x01
    Ret,
    /// 0x0a..=0x11: loads and stores of variables (the opcode)
    Var(u8),
    /// 0x14: message text
    Text(String),
    /// 0x15: jump when the popped value is 0
    Jz(usize),
    /// 0x16
    Jmp(usize),
    /// 0x17: a builtin; the class and then its function are popped
    Call,
    /// 0x18: a line of the scenario (its number and kind)
    Line(u32, u32),
    /// 0x19: a choice: its number is popped, its text follows up to `End`, then go on at the
    /// offset
    Choice(usize),
    /// 0x1a: a line break in a message (0)
    Br(u8),
    /// 0x32
    Int(i32),
    /// 0x33
    Str(String),
    /// 0x34..=0x43: arithmetic and comparisons (0x39, a random number, takes one value)
    Bin(u8),
    Unknown(u8),
}

fn be(code: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(code.get(at..at + 4)?.try_into().ok()?))
}

fn cstr(code: &[u8], at: usize) -> Option<(String, usize)> {
    let end = at + code.get(at..)?.iter().position(|&b| b == 0)?;
    Some((
        String::from_utf8_lossy(&code[at..end]).into_owned(),
        end + 1,
    ))
}

impl Script {
    pub fn parse(name: &str, d: Vec<u8>) -> Result<Script, String> {
        if d.get(..4) != Some(b"SCRE") {
            return Err(format!("{name}: not a SCRE script"));
        }
        let le = |at: usize| {
            d.get(at..at + 4)
                .map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
        };
        let (count, table) = (le(8).unwrap_or(0), le(12).unwrap_or(0x14));
        let base = table + 4 * count;
        if base > d.len() {
            return Err(format!("{name}: the block table runs past the end"));
        }
        Ok(Script {
            name: name.to_string(),
            code: d,
            base,
        })
    }

    /// The instruction at `pc` and where the next one starts.
    pub fn op(&self, pc: usize) -> Option<(Op, usize)> {
        let c = &self.code;
        let op = *c.get(pc)?;
        let at = pc + 1;
        let jump = |at: usize| be(c, at).map(|v| self.base + v as usize);
        Some(match op {
            0x00 => (Op::End, at),
            0x01 => (Op::Ret, at),
            0x0a..=0x11 => (Op::Var(op), at),
            0x14 => {
                let (s, n) = cstr(c, at)?;
                (Op::Text(s), n)
            }
            0x15 => (Op::Jz(jump(at)?), at + 4),
            0x16 => (Op::Jmp(jump(at)?), at + 4),
            0x17 => (Op::Call, at),
            0x18 => (Op::Line(be(c, at)?, be(c, at + 4)?), at + 8),
            0x19 => (Op::Choice(jump(at)?), at + 4),
            0x1a => (Op::Br(*c.get(at)?), at + 1),
            0x32 => (Op::Int(be(c, at)? as i32), at + 4),
            0x33 => {
                let (s, n) = cstr(c, at)?;
                (Op::Str(s), n)
            }
            0x34..=0x43 => (Op::Bin(op), at),
            _ => (Op::Unknown(op), at),
        })
    }

    /// The functions a library or package defines (`11.1` / `12.1`): (class, name, number) ->
    /// how many arguments it takes and where its code starts. A definition is
    /// `flag nargs number 'name' 1 class call` and then the u32 offset of its end.
    pub fn defs(&self) -> HashMap<(i32, String, i32), (usize, usize)> {
        let mut out = HashMap::new();
        let mut st: Vec<Op> = Vec::new();
        let mut pc = self.base;
        while let Some((op, mut next)) = self.op(pc) {
            match op {
                Op::Int(_) | Op::Str(_) => st.push(op),
                Op::Call => {
                    let n = st.len();
                    if n >= 5
                        && let Op::Int(class @ (11 | 12)) = st[n - 1]
                        && st[n - 2] == Op::Int(1)
                    {
                        if let (Op::Str(name), Op::Int(m), Op::Int(nargs)) =
                            (&st[n - 3], &st[n - 4], &st[n - 5])
                        {
                            out.insert(
                                (class, name.clone(), *m),
                                ((*nargs).max(0) as usize, next + 4),
                            );
                        }
                        next += 4;
                    }
                    st.clear();
                }
                _ => st.clear(),
            }
            pc = next;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(code: &[u8]) -> Script {
        let mut d = b"SCRE".to_vec();
        d.extend(0u32.to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d.extend(0x14u32.to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d.extend(code);
        Script::parse("t", d).unwrap()
    }

    #[test]
    fn reads_definitions() {
        // flag 0, 2 args, #103 'img', 1, 11, call, end offset
        let mut c = vec![0x32, 0, 0, 0, 0, 0x32, 0, 0, 0, 2, 0x32, 0, 0, 0, 103, 0x33];
        c.extend(b"img\0");
        c.extend([
            0x32, 0, 0, 0, 1, 0x32, 0, 0, 0, 11, 0x17, 0, 0, 0, 0x20, 0x01, 0x00,
        ]);
        let s = script(&c);
        assert_eq!(s.base, 0x14);
        assert_eq!(
            s.defs().get(&(11, "img".to_string(), 103)),
            Some(&(2, 0x14 + 35))
        );
    }

    #[test]
    fn decodes_jumps_from_the_code_start() {
        let s = script(&[0x16, 0, 0, 0, 5, 0x00]);
        assert_eq!(s.op(0x14), Some((Op::Jmp(0x19), 0x19)));
    }
}
