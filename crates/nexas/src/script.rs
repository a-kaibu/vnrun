//! Compiled scripts (Script.pac/*.bin) of NeXAS releases from about 2010 on.
//! Earlier scripts omit `init` and `str_names` from every block.
//!
//! ```text
//! block    = init:code main:code strings:strs int_names:strs str_names:strs n:u32 n x [u8; 0x44]
//! file     = block { id:u32 block }*          (the functions the script calls, to the end)
//! code     = n:u32 n x { op:u32 arg:u32 }
//! strs     = n:u32 n x NUL-terminated Shift_JIS
//! ```
//!
//! `init` runs once when the file is loaded. Each file carries a copy of every function it calls,
//! so a call needs nothing but the file. The 0x44-byte records are the file's banks: the labels
//! (op 0x1B) the engine can start it at, each with up to 8 parameters that go to local integers.

use std::collections::HashMap;

use crate::sjis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ins {
    pub op: u32,
    pub arg: u32,
}

/// An entry point: label `label`, whose parameter k goes to local integer `vars[k]` (-1: none);
/// a caller passing 99999 leaves it at `defaults[k]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bank {
    pub label: i32,
    pub vars: [i32; 8],
    pub defaults: [i32; 8],
}

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub banks: Vec<Bank>,
    pub init: Vec<Ins>,
    pub code: Vec<Ins>,
    pub strings: Vec<Vec<u8>>,
    pub int_names: Vec<String>,
    pub str_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Script {
    pub main: Block,
    pub funcs: HashMap<u32, Block>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let v = self
            .b
            .get(self.at..self.at + 4)
            .ok_or("script is cut short")?;
        self.at += 4;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    }
    fn code(&mut self) -> Result<Vec<Ins>, String> {
        let n = self.u32()? as usize;
        let end = n
            .checked_mul(8)
            .and_then(|k| k.checked_add(self.at))
            .filter(|&e| e <= self.b.len());
        let end = end.ok_or("code is cut short")?;
        let out = self.b[self.at..end]
            .chunks_exact(8)
            .map(|c| Ins {
                op: u32::from_le_bytes(c[..4].try_into().unwrap()),
                arg: u32::from_le_bytes(c[4..].try_into().unwrap()),
            })
            .collect();
        self.at = end;
        Ok(out)
    }
    fn strs(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            let len = self.b[self.at.min(self.b.len())..]
                .iter()
                .position(|&c| c == 0)
                .ok_or("string is cut short")?;
            out.push(self.b[self.at..self.at + len].to_vec());
            self.at += len + 1;
        }
        Ok(out)
    }
    fn block(&mut self, old: bool) -> Result<Block, String> {
        let init = if old { Vec::new() } else { self.code()? };
        let code = self.code()?;
        let strings = self.strs()?;
        let names = |v: Vec<Vec<u8>>| v.iter().map(|s| sjis::decode(s)).collect();
        let int_names = names(self.strs()?);
        let str_names = if old { Vec::new() } else { names(self.strs()?) };
        let n = self.u32()? as usize;
        if n.checked_mul(0x44)
            .is_none_or(|k| self.at + k > self.b.len())
        {
            return Err("banks are cut short".into());
        }
        let mut banks = Vec::with_capacity(n);
        for _ in 0..n {
            let mut v = [0i32; 17];
            for x in &mut v {
                *x = self.u32()? as i32;
            }
            banks.push(Bank {
                label: v[0],
                vars: v[1..9].try_into().unwrap(),
                defaults: v[9..17].try_into().unwrap(),
            });
        }
        Ok(Block {
            banks,
            init,
            code,
            strings,
            int_names,
            str_names,
        })
    }
}

pub fn parse(b: &[u8]) -> Result<Script, String> {
    parse_format(b, false).or_else(|_| parse_format(b, true))
}

fn parse_format(b: &[u8], old: bool) -> Result<Script, String> {
    let mut r = Reader { b, at: 0 };
    let main = r.block(old)?;
    let mut funcs = HashMap::new();
    while r.at < b.len() {
        let id = r.u32()?;
        funcs.insert(id, r.block(old)?);
    }
    Ok(Script { main, funcs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_old_blocks_without_init_or_string_variable_names() {
        let mut bytes = Vec::new();
        for value in [1u32, 0, 42, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"hello\0");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(b"ret\0");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let script = parse(&bytes).unwrap();
        assert!(script.main.init.is_empty());
        assert_eq!(script.main.code, [Ins { op: 0, arg: 42 }]);
        assert_eq!(script.main.strings, [b"hello".to_vec()]);
        assert_eq!(script.main.int_names, ["ret"]);
        assert!(script.main.str_names.is_empty());
    }
}
