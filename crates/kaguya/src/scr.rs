//! .scr scripts: a 12-byte header (8 magic bytes and the u32 size of the code), then the code.
//! An instruction is a u8 opcode, a u8 length (of the instruction without its strings) and
//! operands; some opcodes are followed by strings (bytes inverted) whose lengths are operands.
//! Jumps go to offsets from the start of the code.

pub struct Script {
    pub name: String,
    pub code: Vec<u8>,
}

/// One instruction: its fixed part (opcode and length included) and its strings.
pub struct Ins {
    pub op: u8,
    pub f: Vec<u8>,
    pub strs: Vec<String>,
    pub next: usize,
}

impl Ins {
    pub fn u8(&self, at: usize) -> u8 {
        self.f.get(at).copied().unwrap_or(0)
    }
    pub fn i16(&self, at: usize) -> i32 {
        self.f
            .get(at..at + 2)
            .map_or(0, |b| i16::from_le_bytes([b[0], b[1]]) as i32)
    }
    pub fn u16(&self, at: usize) -> usize {
        self.f
            .get(at..at + 2)
            .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize)
    }
    pub fn i32(&self, at: usize) -> i32 {
        self.f
            .get(at..at + 4)
            .map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn rgb(&self, at: usize) -> [u8; 3] {
        [self.u8(at), self.u8(at + 1), self.u8(at + 2)]
    }
    pub fn s(&self, i: usize) -> &str {
        self.strs.get(i).map_or("", String::as_str)
    }
}

/// The lengths of the strings after an instruction, from its fixed part.
fn string_lengths(op: u8, f: &[u8]) -> Vec<usize> {
    let b = |i: usize| f.get(i).copied().unwrap_or(0) as usize;
    match op {
        0x0c | 0x0f | 0x10 | 0x12 | 0x2c | 0x3d | 0x48 | 0x49 | 0x50 | 0x52 | 0x56 => vec![b(3)],
        0x13 => vec![if b(3) == 0xff { 0 } else { b(3) }],
        0x15 => vec![b(4)],
        0x2f => vec![b(4) | b(5) << 8],
        0x5e => vec![b(2)],
        0x62 => vec![b(3), b(8)],
        _ => Vec::new(),
    }
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        if b.len() < 12 {
            return Err(format!("{name}: short script"));
        }
        let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let end = (12 + n).min(b.len());
        Ok(Script {
            name: name.to_string(),
            code: b[12..end].to_vec(),
        })
    }

    pub fn decode(&self, pc: usize) -> Option<Ins> {
        let op = *self.code.get(pc)?;
        let size = *self.code.get(pc + 1)? as usize;
        if size < 2 {
            return None;
        }
        let f = self.code.get(pc..pc + size)?;
        let mut next = pc + size;
        let mut strs = Vec::new();
        for len in string_lengths(op, f) {
            let raw = self.code.get(next..next + len)?;
            let raw: Vec<u8> = raw.iter().map(|b| !b).take_while(|&b| b != 0).collect();
            strs.push(encoding_rs::SHIFT_JIS.decode(&raw).0.into_owned());
            next += len;
        }
        Some(Ins {
            op,
            f: f.to_vec(),
            strs,
            next,
        })
    }
}
