//! .mjo scripts: "MajiroObjX1.000" (bytecode XORed with the CRC-32 table) or "MajiroObjV1.000"
//! (plain), then the entry function's offset, the line count, the function table
//! (name hash, offset) and the bytecode.

use std::collections::HashMap;

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

pub const CRC: [u32; 256] = crc_table();

/// The name hash Majiro uses for functions, variables and syscalls (CRC-32).
pub const fn hash(name: &str) -> u32 {
    hash_bytes(name.as_bytes())
}

pub const fn hash_bytes(b: &[u8]) -> u32 {
    let mut c = !0u32;
    let mut i = 0;
    while i < b.len() {
        c = CRC[((c ^ b[i] as u32) & 0xff) as usize] ^ (c >> 8);
        i += 1;
    }
    !c
}

pub struct Script {
    pub name: String,
    pub main: u32,
    pub functions: HashMap<u32, u32>,
    pub code: Vec<u8>,
}

fn u32le(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "short script".to_string())
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        let sig = b.get(..16).ok_or("short script")?;
        let encrypted = match sig {
            b"MajiroObjX1.000\0" => true,
            b"MajiroObjV1.000\0" | b"MjPlainBytecode\0" => false,
            _ => return Err(format!("{name}: not a Majiro script")),
        };
        let main = u32le(b, 16)?;
        let count = u32le(b, 24)? as usize;
        let mut functions = HashMap::new();
        for i in 0..count {
            functions.insert(u32le(b, 28 + i * 8)?, u32le(b, 32 + i * 8)?);
        }
        let at = 28 + count * 8;
        let size = u32le(b, at)? as usize;
        let mut code = b
            .get(at + 4..at + 4 + size)
            .ok_or("short bytecode")?
            .to_vec();
        if encrypted {
            for (i, c) in code.iter_mut().enumerate() {
                *c ^= CRC[(i & 0x3ff) >> 2].to_le_bytes()[i & 3];
            }
        }
        Ok(Script {
            name: name.to_string(),
            main,
            functions,
            code,
        })
    }
}

/// One instruction's operands, as the opcode's encoding says.
#[derive(Clone, Debug, Default)]
pub struct Op {
    pub at: usize,
    pub code: u16,
    pub flags: u16,
    pub hash: u32,
    pub var: i16,
    pub int: i32,
    pub float: f32,
    pub argc: u16,
    pub jump: i32,
    pub text: Vec<u8>,
    pub types: Vec<u8>,
    pub cases: Vec<i32>,
    /// the offset of the next instruction
    pub next: usize,
}

/// The operand letters of an opcode (majiro-py's notation): t types, s string, f flags,
/// h hash, o var offset, 0 zero u32, i int, r float, a argc, j jump, l line, c cases.
pub fn encoding(code: u16) -> Option<&'static str> {
    Some(match code {
        0x100..=0x1a9 => "",
        0x1b0..=0x320 => "fho",
        0x800 => "i",
        0x801 => "s",
        0x802 => "fho",
        0x803 => "r",
        0x80f | 0x810 => "h0a",
        0x829 => "t",
        0x82b => "",
        0x82c..=0x82e => "j",
        0x82f => "",
        0x830..=0x833 | 0x838 | 0x839 => "j",
        0x834 | 0x835 => "ha",
        0x836 => "t",
        0x837 => "fho",
        0x83a => "l",
        0x83b..=0x83d => "j",
        0x83e | 0x83f => "",
        0x840 => "s",
        0x841 => "",
        0x842 => "s",
        0x843 => "j",
        0x844 => "",
        0x845 => "j",
        0x846 => "",
        0x847 => "j",
        0x850 => "c",
        _ => return None,
    })
}

pub fn decode(code: &[u8], at: usize) -> Result<Op, String> {
    let mut p = at;
    let take = |p: &mut usize, n: usize| -> Result<&[u8], String> {
        let s = code
            .get(*p..*p + n)
            .ok_or_else(|| format!("bytecode ends in the instruction at {at:#x}"))?;
        *p += n;
        Ok(s)
    };
    let u16_ = |s: &[u8]| u16::from_le_bytes([s[0], s[1]]);
    let u32_ = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let c = u16_(take(&mut p, 2)?);
    let enc = encoding(c).ok_or_else(|| format!("unknown opcode {c:#x} at {at:#x}"))?;
    let mut op = Op {
        at,
        code: c,
        ..Op::default()
    };
    for e in enc.bytes() {
        match e {
            b't' => {
                let n = u16_(take(&mut p, 2)?) as usize;
                op.types = take(&mut p, n)?.to_vec();
            }
            b's' => {
                let n = u16_(take(&mut p, 2)?) as usize;
                let s = take(&mut p, n)?;
                let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
                op.text = s[..end].to_vec();
            }
            b'f' => op.flags = u16_(take(&mut p, 2)?),
            b'h' => op.hash = u32_(take(&mut p, 4)?),
            b'o' => op.var = u16_(take(&mut p, 2)?) as i16,
            b'0' => {
                take(&mut p, 4)?;
            }
            b'i' => op.int = u32_(take(&mut p, 4)?) as i32,
            b'r' => op.float = f32::from_bits(u32_(take(&mut p, 4)?)),
            b'a' => op.argc = u16_(take(&mut p, 2)?),
            b'j' => op.jump = u32_(take(&mut p, 4)?) as i32,
            b'l' => op.int = u16_(take(&mut p, 2)?) as i32,
            b'c' => {
                let n = u16_(take(&mut p, 2)?) as usize;
                for _ in 0..n {
                    op.cases.push(u32_(take(&mut p, 4)?) as i32);
                }
            }
            _ => unreachable!(),
        }
    }
    op.next = p;
    Ok(op)
}

pub fn mnemonic(c: u16) -> String {
    const BIN: [&str; 18] = [
        "mul", "div", "rem", "add", "sub", "shr", "shl", "cle", "clt", "cge", "cgt", "ceq", "cne",
        "xor", "andl", "orl", "and", "or",
    ];
    const ST: [&str; 11] = [
        "", ".mul", ".div", ".rem", ".add", ".sub", ".shl", ".shr", ".and", ".xor", ".or",
    ];
    const T: [&str; 6] = ["i", "r", "s", "iarr", "rarr", "sarr"];
    match c {
        0x100..=0x18f => format!(
            "{}.{}",
            BIN[(c as usize - 0x100) / 8],
            T[(c & 7) as usize % 6]
        ),
        0x190 => "notl".into(),
        0x198 => "not".into(),
        0x1a0 => "neg.i".into(),
        0x1a1 => "neg.r".into(),
        0x1b0..=0x320 => {
            let (base, name) = match c {
                0x1b0..=0x207 => (0x1b0, "st"),
                0x210..=0x267 => (0x210, "stp"),
                0x270..=0x2c7 => (0x270, "stelem"),
                _ => (0x2d0, "stelemp"),
            };
            let i = (c - base) as usize;
            format!("{name}{}.{}", ST[i / 8], T[(i & 7) % 6])
        }
        0x800 => "ldc.i".into(),
        0x801 => "ldstr".into(),
        0x802 => "ld".into(),
        0x803 => "ldc.r".into(),
        0x80f => "call".into(),
        0x810 => "callp".into(),
        0x829 => "alloca".into(),
        0x82b => "ret".into(),
        0x82c => "br".into(),
        0x82d => "brtrue".into(),
        0x82e => "brfalse".into(),
        0x82f => "pop".into(),
        0x830 => "br.case".into(),
        0x831 => "bne.case".into(),
        0x832 => "bge.case".into(),
        0x833 => "ble.case".into(),
        0x838 => "blt.case".into(),
        0x839 => "bgt.case".into(),
        0x834 => "syscall".into(),
        0x835 => "syscallp".into(),
        0x836 => "argcheck".into(),
        0x837 => "ldelem".into(),
        0x83a => "line".into(),
        0x83b => "bsel.1".into(),
        0x83c => "bsel.3".into(),
        0x83d => "bsel.2".into(),
        0x83e => "conv.i".into(),
        0x83f => "conv.r".into(),
        0x840 => "text".into(),
        0x841 => "proc".into(),
        0x842 => "ctrl".into(),
        0x843 => "bsel.x".into(),
        0x844 => "bsel.clr".into(),
        0x845 => "bsel.4".into(),
        0x846 => "bsel.jmp.4".into(),
        0x847 => "bsel.5".into(),
        0x850 => "switch".into(),
        _ => format!("op.{c:03x}"),
    }
}
