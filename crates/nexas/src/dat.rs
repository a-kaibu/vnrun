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
    while at < b.len() {
        let mut row = Vec::with_capacity(n);
        for &t in &types {
            if t == 1 {
                let len = b[at..]
                    .iter()
                    .position(|&c| c == 0)
                    .ok_or("string is cut short")?;
                row.push(Cell::Str(sjis::decode(&b[at..at + len])));
                at += len + 1;
            } else {
                row.push(Cell::Int(u32_at(at)? as i32));
                at += 4;
            }
        }
        rows.push(row);
    }
    Ok(rows)
}
