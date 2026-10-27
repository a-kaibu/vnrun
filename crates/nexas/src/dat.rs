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
