//! SCD scripts (after "*Pola*" unpacking), as Adv.exe runs them (0x417180).
//!
//! ```text
//! "SCD_" code:u32 (where the code starts), then labels up to `code`: offset:u32 name\0
//! ```
//!
//! Label offsets and jump targets count from `code`. The code is a stack machine; each
//! statement leaves one value that `03` drops:
//!
//! | byte | then | |
//! |------|------|-|
//! | 00 | (text\0 00)* text\0 | lines of text for the top window |
//! | 01 | operand | pop into the variable the operand names |
//! | 02 | operand | push the operand |
//! | 03 | | drop the top (end of a statement) |
//! | 04 | k:u8 t:u32 | k 0: jump; 1 / 3: pop, jump if not 0 / 0; 2 / 4: the same, pushing 1 / 0 when it jumps |
//! | 05 | k:u8 | pop b, a: push a == != < <= > >= b (k 0..5, unsigned) |
//! | 06 | k:u8 | pop b, a: push a + - * / % & \| b (k 0..6); 7: pop b, push -b |
//! | 07.. | | a command of the game (`Engine::command`), its arguments on the stack |
//!
//! Operands are a type byte and a value: 1 u8, 2 u16, 3 the variable u16, 4 the variable
//! named by the variable u16, 5 / 7 the number u16 (a variable's index, for 01), 6 the number in
//! the variable u16, 8 a string\0.

use std::collections::HashMap;

pub struct Script {
    pub name: String,
    pub code: Vec<u8>,
    /// where the code starts in `code`
    pub start: usize,
    pub labels: HashMap<String, usize>,
    /// the labels in order, for listings
    pub label_list: Vec<(String, usize)>,
}

fn u32_(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Script {
    pub fn parse(name: &str, code: Vec<u8>) -> Result<Script, String> {
        if code.len() < 8 || &code[..4] != b"SCD_" {
            return Err(format!("{name}: not an SCD script"));
        }
        let start = u32_(&code, 4).unwrap() as usize;
        if start > code.len() {
            return Err(format!("{name}: code starts past the end"));
        }
        let mut labels = HashMap::new();
        let mut label_list = Vec::new();
        let mut e = 8;
        while e + 4 < start {
            let off = u32_(&code, e).unwrap() as usize;
            let rest = &code[e + 4..start];
            let Some(z) = rest.iter().position(|&c| c == 0) else {
                break;
            };
            let l = String::from_utf8_lossy(&rest[..z]).to_string();
            labels.insert(l.clone(), off);
            label_list.push((l, off));
            e += 4 + z + 1;
        }
        Ok(Script {
            name: name.to_string(),
            code,
            start,
            labels,
            label_list,
        })
    }

    /// The place of a label in `code`.
    pub fn label(&self, l: &str) -> Option<usize> {
        self.labels.get(l).map(|o| self.start + o)
    }
}

/// An operand: its type and what follows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Num(u16),
    Var(u16),
    VarVar(u16),
    Index(u16),
    IndexVar(u16),
    Str(Vec<u8>),
}

/// Reads an operand at `p`; returns it and the place after it.
pub fn operand(b: &[u8], p: usize) -> Option<(Operand, usize)> {
    let t = *b.get(p)?;
    let u16_at =
        |q: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(q)?, *b.get(q + 1)?])) };
    Some(match t {
        1 => (Operand::Num(*b.get(p + 1)? as u16), p + 2),
        2 => (Operand::Num(u16_at(p + 1)?), p + 3),
        3 => (Operand::Var(u16_at(p + 1)?), p + 3),
        4 => (Operand::VarVar(u16_at(p + 1)?), p + 3),
        5 | 7 => (Operand::Index(u16_at(p + 1)?), p + 3),
        6 => (Operand::IndexVar(u16_at(p + 1)?), p + 3),
        8 => {
            let z = b[p + 1..].iter().position(|&c| c == 0)?;
            (Operand::Str(b[p + 1..p + 1 + z].to_vec()), p + 2 + z)
        }
        _ => return None,
    })
}

/// Reads text lines at `p` (after the 00): returns them and the place after them.
pub fn text(b: &[u8], mut p: usize) -> (Vec<Vec<u8>>, usize) {
    let mut lines = Vec::new();
    loop {
        let z = b[p.min(b.len())..]
            .iter()
            .position(|&c| c == 0)
            .map_or(b.len(), |z| p + z);
        lines.push(b[p.min(b.len())..z].to_vec());
        p = z + 1;
        if p < b.len() && b[p] == 0 {
            p += 1;
            continue;
        }
        break;
    }
    (lines, p)
}

/// One line per instruction, for the dump tool.
pub fn listing(s: &Script) -> Vec<String> {
    let sj = |b: &[u8]| encoding_rs::SHIFT_JIS.decode(b).0.to_string();
    let at: HashMap<usize, &str> = s
        .label_list
        .iter()
        .map(|(n, o)| (s.start + o, n.as_str()))
        .collect();
    let b = &s.code;
    let mut out = Vec::new();
    let mut p = s.start;
    while p < b.len() {
        if let Some(l) = at.get(&p) {
            out.push(format!("{l}:"));
        }
        let o = p - s.start;
        let op = b[p];
        p += 1;
        let line = match op {
            0 => {
                let (lines, q) = text(b, p);
                p = q;
                let t: Vec<String> = lines.iter().map(|l| sj(l)).collect();
                format!("text {t:?}")
            }
            1 | 2 => match operand(b, p) {
                Some((v, q)) => {
                    p = q;
                    let v = match v {
                        Operand::Str(s) => format!("{:?}", sj(&s)),
                        v => format!("{v:?}"),
                    };
                    format!("{} {v}", if op == 1 { "set" } else { "push" })
                }
                None => break,
            },
            3 => "end".into(),
            4 => {
                let (Some(&k), Some(t)) = (b.get(p), u32_(b, p + 1)) else {
                    break;
                };
                p += 5;
                format!("jump{k} {t}")
            }
            5 | 6 => {
                let Some(&k) = b.get(p) else { break };
                p += 1;
                format!("{} {k}", if op == 5 { "compare" } else { "compute" })
            }
            _ => format!("command {op:#04x}"),
        };
        out.push(format!("{o:6} {line}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_text() {
        let mut b = b"SCD_".to_vec();
        b.extend(19u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(b"#start\0");
        b.extend([0, b'a', 0, 0, b'b', 0, 3]);
        let s = Script::parse("x", b).unwrap();
        assert_eq!(s.label("#start"), Some(19));
        let (lines, p) = text(&s.code, 20);
        assert_eq!(lines, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(s.code[p], 3);
    }
}
