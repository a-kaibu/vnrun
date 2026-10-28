//! The scenario: `<game>_sce.dat`, decrypted, is a header (where the code ends, where the
//! strings start), the bytecode from 0x0C and the strings (Shift-JIS, each ending in 0).
//!
//! An instruction is an opcode byte and its operands. Their layout comes from the game's
//! program (the switch of its interpreter, read once per release family with
//! tools in the scratch notes): B a byte, H 16 bits, I 32 bits, V a value (0x3D local or 0x3E
//! global variable and its 16-bit number, otherwise a 32-bit number), C a count and that many
//! 32-bit string numbers. Opcodes 0 and 1 (and a few unused ones) do nothing; message
//! instructions leave their record numbers' high bytes as such no-ops.

use std::collections::HashMap;

/// The operands of the opcodes of the 2008 to 2010 releases (息子の友達に犯されて, 贄姫).
const FAMILY_2008: &[(u8, &str)] = &[
    (0x02, "B"),
    (0x03, "B"),
    (0x04, ""),
    (0x0b, "I"),
    (0x0c, "I"),
    (0x0d, ""),
    (0x0e, ""),
    (0x14, "II"),
    (0x15, "H"),
    (0x19, "BHI"),
    (0x1a, "BHI"),
    (0x1b, "BHI"),
    (0x1c, "BHI"),
    (0x1d, "BHI"),
    (0x1e, "BHI"),
    (0x1f, "HI"),
    (0x20, "HH"),
    (0x21, "HI"),
    (0x22, "BHII"),
    (0x23, "B"),
    (0x24, "BHII"),
    (0x25, "BHII"),
    (0x26, "BHII"),
    (0x27, "BHII"),
    (0x28, "BHII"),
    (0x29, "BHII"),
    (0x2a, "BHII"),
    (0x2b, "BHII"),
    (0x2c, "BHII"),
    (0x2d, "BHII"),
    (0x2e, "BHII"),
    (0x2f, "BHII"),
    (0x30, "BHBH"),
    (0x31, "BHBH"),
    (0x32, "BHBH"),
    (0x33, "BHBH"),
    (0x34, "BHBH"),
    (0x35, "BHBH"),
    (0x36, "BHIBH"),
    (0x37, "BHBHI"),
    (0x38, "BHBHI"),
    (0x39, "BHBHI"),
    (0x3a, "BHBHI"),
    (0x3b, "BHBHI"),
    (0x3c, "BHBHI"),
    (0x3f, "II"),
    (0x46, "I"),
    (0x47, ""),
    (0x48, "BII"),
    (0x49, "BI"),
    (0x4a, "BI"),
    (0x4b, "BII"),
    (0x4c, "B"),
    (0x4d, "B"),
    (0x4e, "IIIIIII"),
    (0x4f, "H"),
    (0x50, "HHHH"),
    (0x51, "I"),
    (0x53, "B"),
    (0x54, ""),
    (0x55, ""),
    (0x56, "BBBHH"),
    (0x57, "IBBB"),
    (0x58, "I"),
    (0x5a, "B"),
    (0x5b, ""),
    (0x5c, "H"),
    (0x5d, "I"),
    (0x5e, "I"),
    (0x5f, ""),
    (0x60, "HI"),
    (0x61, "H"),
    (0x62, "I"),
    (0x63, "I"),
    (0x64, "H"),
    (0x65, ""),
    (0x66, ""),
    (0x67, ""),
    (0x68, ""),
    (0x69, ""),
    (0x6a, ""),
    (0x6b, "B"),
    (0x6c, "I"),
    (0x6d, "I"),
    (0x6e, ""),
    (0x6f, "HHHHHI"),
    (0x70, "HHHHHI"),
    (0x71, "HI"),
    (0x72, "H"),
    (0x73, ""),
    (0x76, "B"),
    (0x77, "B"),
    (0x78, "III"),
    (0x79, ""),
    (0x7a, "IIIIII"),
    (0x7b, "BH"),
    (0x7c, ""),
    (0x8c, "II"),
    (0x8d, "IIII"),
    (0x8e, "IIIIBH"),
    (0x8f, "I"),
    (0x90, "IIII"),
    (0x91, ""),
    (0x92, "BHBHII"),
    (0x93, "II"),
    (0x94, ""),
    (0x95, "VVVV"),
    (0x96, "B"),
    (0x97, "VVVV"),
    (0x98, "IVVVVVV"),
    (0x99, "IVVVVVV"),
    (0x9a, "IIIIIIIII"),
    (0x9b, "HI"),
    (0x9c, "HI"),
    (0x9d, "HI"),
    (0x9e, "BH"),
    (0x9f, "BH"),
    (0xb4, "C"),
    (0xb5, ""),
    (0xb6, ""),
    (0xb7, ""),
    (0xb8, ""),
    (0xc8, "C"),
    (0xc9, "B"),
    (0xe6, "IIIH"),
    (0xe7, "IIH"),
    (0xe8, "IIH"),
    (0xe9, "IH"),
    (0xf0, "II"),
];

/// What the 2009 release (愛娘という名の玩具) does differently: its voiced and named messages
/// start with one more number (0 in the trial), 41 waits and b9 takes nothing.
const FAMILY_2009: &[(u8, &str)] = &[(0xe6, "IIIII"), (0xe7, "IIII"), (0x41, "I"), (0xb9, "")];

/// The 2011 release (NTR彼女はいじめられっ娘) renumbered the opcodes (read from its interpreter
/// and checked against the whole trial scenario). V values here always have a kind byte;
/// Z is a label (a string in the code), X the transition (kind, time, then 5, 4 or 16 more bytes
/// for kinds 28, 46 and 47).
const FAMILY_2011: &[(u8, &str)] = &[
    (0x02, "B"),
    (0x03, ""),
    (0x04, "BBBIII"),
    (0x0a, "Z"),
    (0x0b, "I"),
    (0x0c, "I"),
    (0x0d, ""),
    (0x0e, ""),
    (0x14, "I"),
    (0x15, "H"),
    (0x19, "BHV"),
    (0x1a, "BHV"),
    (0x1b, "BHV"),
    (0x1c, "BHV"),
    (0x1d, "BHV"),
    (0x1e, "BHV"),
    (0x1f, "I"),
    (0x23, "BHV"),
    (0x24, "BHVI"),
    (0x25, "BHVI"),
    (0x26, "BHVI"),
    (0x27, "BHVI"),
    (0x28, "BHVI"),
    (0x29, "BHVI"),
    (0x2a, "BHVI"),
    (0x2b, "BHVI"),
    (0x2c, "BHVI"),
    (0x2d, "BHVI"),
    (0x2e, "BHVI"),
    (0x2f, "BHVI"),
    (0x37, "BHVI"),
    (0x38, "BHVI"),
    (0x39, "BHVI"),
    (0x3a, "BHVI"),
    (0x3b, "BHVI"),
    (0x3c, "BHVI"),
    (0x46, "I"),
    (0x47, "I"),
    (0x48, "BB"),
    (0x49, "I"),
    (0x4a, "BI"),
    (0x4b, "BI"),
    (0x4c, "B"),
    (0x4d, ""),
    (0x4e, "BB"),
    (0x4f, "BBI"),
    (0x50, "BI"),
    (0x51, "BHHVVVVVB"),
    (0x52, "BVVVB"),
    (0x53, ""),
    (0x54, "X"),
    (0x55, "III"),
    (0x56, "H"),
    (0x58, ""),
    (0x59, "II"),
    (0x5a, ""),
    (0x5b, ""),
    (0x5c, "I"),
    (0x5d, "BBB"),
    (0x5e, "BBB"),
    (0x5f, "BBB"),
    (0x60, "II"),
    (0x61, "IIIII"),
    (0x63, "IIII"),
    (0x64, "IIII"),
    (0x65, "IIII"),
    (0x6e, "B"),
    (0x6f, ""),
    (0x70, "B"),
    (0x71, "I"),
    (0x72, ""),
    (0x73, "HI"),
    (0x74, "H"),
    (0x75, ""),
    (0x76, "II"),
    (0x77, ""),
    (0x78, ""),
    (0x82, "I"),
    (0x83, "I"),
    (0x84, "I"),
    (0x85, ""),
    (0x86, ""),
    (0x88, "B"),
    (0x89, ""),
    (0x8a, ""),
    (0x8b, "BH"),
    (0x8d, "BH"),
    (0x8e, "BH"),
    (0x8f, "BH"),
    (0xbe, "C"),
    (0xbf, "B"),
    (0xc1, ""),
    (0xc2, ""),
    (0xc3, ""),
    (0xc8, "IIII"),
    (0xc9, "III"),
    (0xca, "III"),
    (0xcb, "II"),
    (0xd2, ""),
];

/// What the 2011 opcodes are in the 2008 numbering the engine runs (the rest do nothing).
const REMAP_2011: &[(u8, u8)] = &[
    (0x0b, 0x0b), // goto, gosub, return
    (0x0c, 0x0c),
    (0x0d, 0x0d),
    (0x19, 0x19), // variable op= value
    (0x1a, 0x1a),
    (0x1b, 0x1b),
    (0x1c, 0x1c),
    (0x1d, 0x1d),
    (0x1e, 0x1e),
    (0x24, 0x24), // if variable compares to a value: goto, then gosub
    (0x25, 0x25),
    (0x26, 0x26),
    (0x27, 0x27),
    (0x28, 0x28),
    (0x29, 0x29),
    (0x2a, 0x2a),
    (0x2b, 0x2b),
    (0x2c, 0x2c),
    (0x2d, 0x2d),
    (0x2e, 0x2e),
    (0x2f, 0x2f),
    (0x37, 0x24), // the same handler as 24 to 29 in this release
    (0x38, 0x25),
    (0x39, 0x26),
    (0x3a, 0x27),
    (0x3b, 0x28),
    (0x3c, 0x29),
    (0x46, 0x46), // background
    (0x4a, 0x49), // character in a slot
    (0x4c, 0x4c), // character away
    (0x54, 0x4d), // transition (kind, time)
    (0x6e, 0x5a), // music
    (0x6f, 0x5b), // music off
    (0x71, 0x5d), // sound
    (0x73, 0x60), // looping sound on a channel
    (0x74, 0x61), // and off
    (0x82, 0x64), // pause
    (0xbe, 0xc8), // choice
    (0xbf, 0xc9), // its case
    (0xc8, 0xe6), // voiced, named message
    (0xc9, 0xe7), // named message
    (0xca, 0xe8), // voiced message
    (0xcb, 0xe9), // message
];

/// A release family: its operand table (changes to [`FAMILY_2008`], or a table of its own),
/// where its code starts and how its opcodes map onto the 2008 ones.
struct Family {
    name: &'static str,
    own: bool,
    table: &'static [(u8, &'static str)],
    start: usize,
    remap: &'static [(u8, u8)],
}

/// The releases whose opcodes we know.
const FAMILIES: &[Family] = &[
    Family {
        name: "2008",
        own: false,
        table: &[],
        start: 0x0c,
        remap: &[],
    },
    Family {
        name: "2009",
        own: false,
        table: FAMILY_2009,
        start: 0x0c,
        remap: &[],
    },
    Family {
        name: "2011",
        own: true,
        table: FAMILY_2011,
        start: 0x08,
        remap: REMAP_2011,
    },
];

/// An operand.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    N(i64),
    /// a variable: 0x3D local, 0x3E global (or another kind byte), and its number
    Var(u8, u16),
    List(Vec<i64>),
}

impl Arg {
    pub fn n(&self) -> i64 {
        match self {
            Arg::N(n) => *n,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Insn {
    pub at: u32,
    pub op: u8,
    pub args: Vec<Arg>,
}

pub struct Program {
    /// the release family whose opcodes fit the scenario ("2008" or "2009")
    pub family: &'static str,
    pub code: Vec<Insn>,
    /// the instruction at (or next after) each code offset
    pub index: HashMap<u32, usize>,
    pub strings: Vec<String>,
}

impl Program {
    /// Decodes the scenario with the first family whose opcodes fit it to the end.
    pub fn parse(d: &[u8]) -> Result<Program, String> {
        let mut first = None;
        for f in FAMILIES {
            let mut table: HashMap<u8, &str> = HashMap::new();
            if !f.own {
                table.extend(FAMILY_2008.iter().copied());
            }
            table.extend(f.table.iter().copied());
            match Program::parse_with(d, &table, f.start) {
                Ok(mut p) => {
                    p.family = f.name;
                    if !f.remap.is_empty() {
                        let map: HashMap<u8, u8> = f.remap.iter().copied().collect();
                        for i in &mut p.code {
                            // 0 does nothing in every release
                            i.op = map.get(&i.op).copied().unwrap_or(0);
                        }
                    }
                    return Ok(p);
                }
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.unwrap_or_default())
    }

    fn parse_with(d: &[u8], table: &HashMap<u8, &str>, start: usize) -> Result<Program, String> {
        let word = |at: usize| -> Result<usize, String> {
            d.get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                .ok_or_else(|| "the scenario is cut short".to_string())
        };
        let (end, strs) = (word(0)?, word(4)?);
        if end > d.len() || strs > d.len() || end < start {
            return Err("not a Crowd scenario".into());
        }
        let strings = d[strs..]
            .split(|&c| c == 0)
            .map(nexas::sjis::decode)
            .collect();
        let mut code = Vec::new();
        let mut index = HashMap::new();
        let mut pending = Vec::new();
        let mut p = start;
        while p < end {
            let at = p as u32;
            let op = d[p];
            p += 1;
            if op <= 1 {
                pending.push(at);
                continue;
            }
            let spec = table
                .get(&op)
                .ok_or_else(|| format!("unknown opcode {op:02x} at {at:#x}"))?;
            let mut args = Vec::new();
            let take = |p: &mut usize, n: usize| -> Result<u64, String> {
                let b = d
                    .get(*p..*p + n)
                    .ok_or_else(|| format!("opcode {op:02x} at {at:#x} runs past the end"))?;
                *p += n;
                Ok(b.iter().rev().fold(0u64, |v, &c| (v << 8) | c as u64))
            };
            for c in spec.chars() {
                match c {
                    'B' => args.push(Arg::N(take(&mut p, 1)? as i64)),
                    'H' => args.push(Arg::N(take(&mut p, 2)? as i64)),
                    'I' => args.push(Arg::N(take(&mut p, 4)? as u32 as i32 as i64)),
                    'V' => {
                        let k = take(&mut p, 1)? as u8;
                        if k == 0x3d || k == 0x3e {
                            args.push(Arg::Var(k, take(&mut p, 2)? as u16));
                        } else {
                            args.push(Arg::N(take(&mut p, 4)? as u32 as i32 as i64));
                        }
                    }
                    'C' => {
                        let n = take(&mut p, 1)? as usize;
                        let mut v = Vec::with_capacity(n);
                        for _ in 0..n {
                            v.push(take(&mut p, 4)? as i64);
                        }
                        args.push(Arg::List(v));
                    }
                    'Z' => {
                        let n = d[p.min(d.len())..]
                            .iter()
                            .position(|&c| c == 0)
                            .ok_or_else(|| format!("label at {at:#x} runs past the end"))?;
                        p += n + 1;
                    }
                    'X' => {
                        let kind = take(&mut p, 1)?;
                        args.push(Arg::N(kind as i64));
                        args.push(Arg::N(take(&mut p, 4)? as u32 as i32 as i64));
                        let more = match kind {
                            0x28 => 5,
                            0x46 => 4,
                            0x47 => 16,
                            _ => 0,
                        };
                        if p + more > d.len() {
                            return Err(format!("opcode {op:02x} at {at:#x} runs past the end"));
                        }
                        p += more;
                    }
                    _ => {}
                }
            }
            for a in pending.drain(..) {
                index.insert(a, code.len());
            }
            index.insert(at, code.len());
            code.push(Insn { at, op, args });
        }
        if p != end {
            return Err(format!("the code runs past its end ({p:#x} > {end:#x})"));
        }
        for a in pending.drain(..) {
            index.insert(a, code.len());
        }
        index.insert(end as u32, code.len());
        Ok(Program {
            family: "",
            code,
            index,
            strings,
        })
    }

    pub fn string(&self, n: i64) -> &str {
        usize::try_from(n)
            .ok()
            .and_then(|i| self.strings.get(i))
            .map_or("", |s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_little_program() {
        let mut d = vec![0u8; 12];
        // e9 (text 1, record 0, two no-op bytes), 0b goto 0x0c, then the strings
        let code: &[u8] = &[0xe9, 1, 0, 0, 0, 0, 0, 0, 0, 0x0b, 0x0c, 0, 0, 0];
        d.extend(code);
        let end = d.len() as u32;
        d.extend(b"zero\0one\0");
        d[0..4].copy_from_slice(&end.to_le_bytes());
        d[4..8].copy_from_slice(&end.to_le_bytes());
        let p = Program::parse(&d).unwrap();
        assert_eq!(p.code.len(), 2);
        assert_eq!(p.code[0].op, 0xe9);
        assert_eq!(p.string(p.code[0].args[0].n()), "one");
        assert_eq!(p.index[&0x15], 1);
        assert_eq!(p.code[1].args[0], Arg::N(0x0c));
    }
}
