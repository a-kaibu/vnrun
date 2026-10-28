//! The 2008 GAME_SYS.exe builds its characters in code: for each name the scripts use
//! ("kaz_0105_s") it registers a body ("kaz_01_s", a whole-screen picture without a face), a
//! face ("kaz0005_s", a small picture without a place) and where the face goes. The calls
//! look alike:
//!
//! ```text
//! mov r, y; push r; mov r, x; push r; push 0; push 0;
//! push face; push body; push name; mov ecx, table
//! ```
//!
//! so the table is read back from the program's bytes.

use std::collections::HashMap;

/// name → (body, face, face x, face y)
pub type Characters = HashMap<String, (String, String, i32, i32)>;

fn u32at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The sections of a PE file: (virtual address with the image base, file offset, size).
fn sections(b: &[u8]) -> Option<Vec<(u32, usize, usize)>> {
    let pe = u32at(b, 0x3c)? as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let n = u16::from_le_bytes(b.get(pe + 6..pe + 8)?.try_into().ok()?) as usize;
    let opt = u16::from_le_bytes(b.get(pe + 20..pe + 22)?.try_into().ok()?) as usize;
    let base = u32at(b, pe + 24 + 28)?;
    let mut out = Vec::new();
    for i in 0..n {
        let o = pe + 24 + opt + 40 * i;
        let va = u32at(b, o + 12)?;
        let raw_size = u32at(b, o + 16)? as usize;
        let raw = u32at(b, o + 20)? as usize;
        out.push((base.wrapping_add(va), raw, raw_size));
    }
    Some(out)
}

pub fn characters(b: &[u8]) -> Characters {
    let mut out = Characters::new();
    let Some(secs) = sections(b) else {
        return out;
    };
    // a string the code points at
    let string = |va: u32| -> Option<String> {
        let &(v, raw, size) = secs
            .iter()
            .find(|&&(v, _, s)| va >= v && va < v + s as u32)?;
        let at = raw + (va - v) as usize;
        let end = b.get(at..raw + size)?.iter().position(|&c| c == 0)?;
        let s = b.get(at..at + end)?;
        if s.is_empty() || s.len() > 40 || !s.iter().all(|c| c.is_ascii_graphic()) {
            return None;
        }
        Some(String::from_utf8_lossy(s).into_owned())
    };
    let mov = |at: usize| -> Option<i32> {
        // mov r32, imm32; push r32
        let op = *b.get(at)?;
        let push = *b.get(at + 5)?;
        ((0xb8..=0xbf).contains(&op) && (0x50..=0x57).contains(&push))
            .then(|| u32at(b, at + 1).map(|v| v as i32))?
    };
    let mut at = 0;
    while at + 40 < b.len() {
        // push 0; push 0; push face; push body; push name; mov ecx, ...
        if b[at..at + 5] == [0x6a, 0, 0x6a, 0, 0x68]
            && b[at + 9] == 0x68
            && b[at + 14] == 0x68
            && b[at + 19] == 0xb9
            && at >= 12
            && let (Some(y), Some(x)) = (mov(at - 12), mov(at - 6))
            && let (Some(face), Some(body), Some(name)) = (
                u32at(b, at + 5).and_then(string),
                u32at(b, at + 10).and_then(string),
                u32at(b, at + 15).and_then(string),
            )
        {
            out.insert(name.to_lowercase(), (body, face, x, y));
            at += 24;
            continue;
        }
        at += 1;
    }
    out
}
