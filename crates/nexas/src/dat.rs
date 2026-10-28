//! The tables of Config.pac (*.dat): visuals, buttons, characters, ...
//!
//! ```text
//! columns:u32 columns x type:u32 (1: NUL-terminated Shift_JIS, 2: i32)  rows to the end
//! ```

use crate::sjis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Int(i32),
    Str(String),
}

impl Cell {
    pub fn int(&self) -> i32 {
        match self {
            Cell::Int(i) => *i,
            Cell::Str(_) => 0,
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Cell::Str(s) => s,
            Cell::Int(_) => "",
        }
    }
}

/// 2006 (angel breath) tables have no header: fixed records of NUL-padded 64-byte strings (S)
/// and i32s (I), given here per file and laid out as the later tables' columns the engine
/// reads. Visual: three numbers, the picture, the one laid over it and where. Stand: a
/// number, the body (.fil), the face, the animation (.ani), the expression and where it goes.
/// Event: the name, the file and ten numbers (file put at column 4, as later).
pub fn parse_2006(name: &str, b: &[u8]) -> Option<Vec<Vec<Cell>>> {
    let layout: &[u8] = match name.to_ascii_lowercase().as_str() {
        "visual.dat" => b"IIISSII",
        "stand.dat" => b"ISSSSII",
        "event.dat" => b"SSIIIIIIIIII",
        _ => return None,
    };
    let size: usize = layout.iter().map(|&c| if c == b'S' { 64 } else { 4 }).sum();
    if b.is_empty() || b.len() % size != 0 {
        return None;
    }
    let rows = b
        .chunks_exact(size)
        .map(|rec| {
            let mut at = 0;
            let mut row: Vec<Cell> = layout
                .iter()
                .map(|&c| {
                    if c == b'S' {
                        let s = &rec[at..at + 64];
                        at += 64;
                        let end = s.iter().position(|&c| c == 0).unwrap_or(64);
                        Cell::Str(sjis::decode(&s[..end]))
                    } else {
                        at += 4;
                        Cell::Int(i32::from_le_bytes(rec[at - 4..at].try_into().unwrap()))
                    }
                })
                .collect();
            if layout.len() == 12 {
                // Event: name, 0, 0, 0, file, numbers
                let file = row.remove(1);
                row.splice(1..1, [Cell::Int(0), Cell::Int(0), Cell::Int(0), file]);
            }
            row
        })
        .collect();
    Some(rows)
}

pub fn parse(b: &[u8]) -> Result<Vec<Vec<Cell>>, String> {
    let u32_at = |at: usize| -> Result<u32, String> {
        let v = b.get(at..at + 4).ok_or("table is cut short")?;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    };
    let n = u32_at(0)? as usize;
    if n == 0 || n > 1024 {
        return Err(format!("{n} columns"));
    }
    let types: Vec<u32> = (0..n)
        .map(|i| u32_at(4 + 4 * i))
        .collect::<Result<_, _>>()?;
    if types.iter().any(|&t| t > 16) {
        return Err("not a table".into());
    }
    let mut at = 4 + 4 * n;
    let mut rows = Vec::new();
    // A cut short last row is left out (さかあがりハリケーン's Event.dat).
    'rows: while at < b.len() {
        let mut row = Vec::with_capacity(n);
        for &t in &types {
            if t == 1 {
                let Some(len) = b[at.min(b.len())..].iter().position(|&c| c == 0) else {
                    break 'rows;
                };
                row.push(Cell::Str(sjis::decode(&b[at..at + len])));
                at += len + 1;
            } else {
                let Ok(v) = u32_at(at) else {
                    break 'rows;
                };
                row.push(Cell::Int(v as i32));
                at += 4;
            }
        }
        rows.push(row);
    }
    Ok(rows)
}
