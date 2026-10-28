//! The `.mes` scripts (and `LIBLARY.LIB`): two u32 counts of message marks and their
//! addresses, then stack code. Operands are big-endian; addresses count from the code start.

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Yield,
    Ret,
    /// load (0x02..=0x09) / store (0x0c..=0x13) of a variable kind
    Load(u8),
    Store(u8),
    /// shown text; the first form is the packed one
    Text(Vec<u8>),
    Jz(u32),
    Jmp(u32),
    /// registers a library routine at an address
    LibReg(u32),
    LibCall,
    Syscall,
    MsgId(i32),
    MsgId2(i32),
    Choice(u32),
    Escape(u8),
    Int(i32),
    Str(Vec<u8>),
    /// 0x34..=0x43: add sub mul div mod rand logand logor binand binor lt gt le ge eq neq
    Bin(u8),
    /// 0xfa..=0xff
    Mark(u8),
}

pub struct Mes {
    pub code: Vec<u8>,
    /// the message marks (addresses of MsgId ops)
    pub marks: Vec<u32>,
}

impl Mes {
    pub fn parse(b: &[u8]) -> Result<Mes, String> {
        let u = |i: usize| -> Result<u32, String> {
            Ok(u32::from_le_bytes(
                b.get(i..i + 4).ok_or("short .mes")?.try_into().unwrap(),
            ))
        };
        let n = u(0)? as usize + u(4)? as usize;
        let start = 8 + 4 * n;
        if start > b.len() {
            return Err("bad .mes header".into());
        }
        let marks = (0..n).map(|i| u(8 + 4 * i)).collect::<Result<_, _>>()?;
        Ok(Mes {
            code: b[start..].to_vec(),
            marks,
        })
    }

    /// The op at `pc` and the address after it.
    pub fn op(&self, pc: usize) -> Result<(Op, usize), String> {
        let c = &self.code;
        let op = *c
            .get(pc)
            .ok_or_else(|| format!("pc {pc:#x} past the end"))?;
        let int = |p: usize| -> Result<i32, String> {
            Ok(i32::from_be_bytes(
                c.get(p..p + 4)
                    .ok_or_else(|| format!("short operand at {p:#x}"))?
                    .try_into()
                    .unwrap(),
            ))
        };
        let string = |p: usize| -> Result<(Vec<u8>, usize), String> {
            let n = c[p..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| format!("unterminated string at {p:#x}"))?;
            Ok((c[p..p + n].to_vec(), p + n + 1))
        };
        let p = pc + 1;
        Ok(match op {
            0x00 => (Op::Yield, p),
            0x01 => (Op::Ret, p),
            0x02..=0x09 => (Op::Load(op), p),
            0x0c..=0x13 => (Op::Store(op), p),
            0x0a => {
                let (s, q) = string(p)?;
                (Op::Text(unpack_text(&s)), q)
            }
            0x0b => {
                let (s, q) = string(p)?;
                (Op::Text(s), q)
            }
            0x14 => (Op::Jz(int(p)? as u32), p + 4),
            0x15 => (Op::Jmp(int(p)? as u32), p + 4),
            0x16 => (Op::LibReg(int(p)? as u32), p + 4),
            0x17 => (Op::LibCall, p),
            0x18 => (Op::Syscall, p),
            0x19 => (Op::MsgId(int(p)?), p + 4),
            0x1a => (Op::MsgId2(int(p)?), p + 4),
            0x1b => (Op::Choice(int(p)? as u32), p + 4),
            0x1c => (Op::Escape(*c.get(p).ok_or("short escape")?), p + 1),
            0x32 => (Op::Int(int(p)?), p + 4),
            0x33 => {
                let (s, q) = string(p)?;
                (Op::Str(s), q)
            }
            0x34..=0x43 => (Op::Bin(op), p),
            0xfa..=0xff => (Op::Mark(op), p),
            _ => return Err(format!("unknown op {op:#04x} at {pc:#x}")),
        })
    }
}

/// The packed text form: a byte outside the Shift_JIS lead ranges stands for the two-byte
/// character `b + 0x829e` (VNTextPatch: `b - 0x7d62` as 16 bits), so 0x01 is ぁ (0x829f).
pub fn unpack_text(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        i += 1;
        if (0x81..0xa0).contains(&b) || (0xe0..0xf0).contains(&b) {
            out.push(b);
            if let Some(&t) = s.get(i) {
                out.push(t);
                i += 1;
            }
        } else {
            let c = (b as i32 - 0x7d62) as u16;
            out.extend([(c >> 8) as u8, c as u8]);
        }
    }
    out
}
