//! The records of an `.asb` script: u32 op, u32 length (with these 8 bytes), then the
//! arguments: 32 bit numbers, or for the text ops four numbers and a Shift_JIS string.

pub struct Rec {
    pub op: u32,
    pub a: Vec<i32>,
    pub s: String,
}

pub const TEXT: u32 = 0x2a;
pub const NAME: u32 = 0x29;
pub const CHAPTER: u32 = 0x04;
pub const SE: u32 = 0x75;
pub const VOICE: u32 = 0x79;

pub fn parse(d: &[u8]) -> Vec<Rec> {
    let mut out = Vec::new();
    let mut q = 0;
    while q + 8 <= d.len() {
        let op = u32::from_le_bytes(d[q..q + 4].try_into().unwrap());
        let len = u32::from_le_bytes(d[q + 4..q + 8].try_into().unwrap()) as usize;
        let Some(end) = q.checked_add(len).filter(|&e| len >= 8 && e <= d.len()) else {
            break;
        };
        let body = &d[q + 8..end];
        let texty = matches!(op, TEXT | NAME | CHAPTER | SE | VOICE);
        let n = if texty {
            body.len().min(16)
        } else {
            body.len()
        };
        let a = body[..n / 4 * 4]
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let s = if texty && body.len() > 16 {
            let t = &body[16..];
            nexas::sjis::decode(&t[..t.iter().position(|&b| b == 0).unwrap_or(t.len())])
        } else {
            String::new()
        };
        out.push(Rec { op, a, s });
        q = end;
    }
    out
}
